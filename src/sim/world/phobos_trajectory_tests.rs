//! `Trajectory.Straight` flight through the production bullet store and the
//! world's projectile collision owner.
use super::*;
use crate::sim::projectile::{
    ProjectileCollisionPolicy, ProjectileSpawn, ProjectileTrajectory, ProjectileVisualState,
    TargetExpiryPolicy,
};
use crate::sim::world::LifecycleOutput;

const HEAD: &str = "[General]\n[Countries]\n0=Americans\n1=Russians\n\
    [InfantryTypes]\n[VehicleTypes]\n0=TGT\n1=BLOCK\n[AircraftTypes]\n\
    [BuildingTypes]\n\
    [Warheads]\n0=WH\n1=PSST\n\
    [WH]\n[PSST]\n\
    [TGT]\nStrength=100\nArmor=light\nSpeed=4\nPrimary=GUN\n\
    Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
    [BLOCK]\nStrength=100\nArmor=light\nSpeed=4\nPrimary=GUN\n\
    Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n";
const GUN: &str = "[GUN]\nDamage=10\nROF=20\nRange=8\nProjectile=PROJ\nSpeed=20\nWarhead=WH\n";
const TAIL: &str = "";

struct Scene {
    sim: Simulation,
    rules: RuleSet,
}

fn fixture(proj_keys: &str) -> Scene {
    fixture_burst(proj_keys, 1)
}

fn fixture_burst(proj_keys: &str, burst: i32) -> Scene {
    let text = format!("{HEAD}{GUN}Burst={burst}\n[PROJ]\n{proj_keys}{TAIL}");
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(&text))
        .expect("trajectory rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x0046_66E0);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session.set_binary_frame_for_test(0);
    Scene { sim, rules }
}

/// A Straight trajectory whose BulletType also carries `LaserTrail.Types`
/// in its ART section.
fn fixture_with_trails() -> Scene {
    let rules_text = format!("{HEAD}{GUN}[PROJ]\nTrajectory=Straight\nTrajectory.Speed=16\n");
    let art = crate::rules::ini_parser::IniFile::from_str(
        "[PROJ]\nLaserTrail.Types=TRAIL\n[LaserTrailTypes]\n0=TRAIL\n\
         [TRAIL]\nSegmentLength=1\nFadeDuration=3\n",
    );
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(&rules_text),
        &art,
    )
    .expect("trajectory trail rules");
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x0046_66E0);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session.set_binary_frame_for_test(0);
    Scene { sim, rules }
}

/// Fire the fixture bullet at `origin`, aimed at `target_position`.
fn fire(
    scene: &mut Scene,
    target: ProjectileTarget,
    origin: ProjectileCoord,
    target_position: ProjectileCoord,
) -> u64 {
    let projectile_type = scene.sim.interner.intern("PROJ");
    let warhead = scene.sim.interner.intern("WH");
    let spawn = ProjectileSpawn {
        native_unique_id: 1,
        line_trail: None,
        flat: false,
        source_id: 0,
        origin,
        target,
        initial_target_position: target_position,
        payload: ProjectilePayload::new(10, warhead, Some(projectile_type), false),
        speed_leptons_per_frame: 16,
        velocity: ProjectileVelocity::new(0, 0, 0),
        trajectory: ProjectileTrajectory::Ballistic,
        guidance: None,
        visual: ProjectileVisualState::new(0, 0, 0),
        arm_frames: 0,
        fuse_frames: None,
        ranged_fuse: false,
        tracks_target: false,
        target_expiry: TargetExpiryPolicy::DetonateAtLastKnown,
        collision: ProjectileCollisionPolicy::NONE,
    };
    let id = scene.sim.projectiles.spawn_at(1, 0, spawn, origin);
    let weapon = scene.rules.weapon("GUN").expect("weapon").clone();
    scene.sim.init_phobos_trajectory(id, &scene.rules, &weapon);
    id
}

fn state_of<'a>(scene: &'a Scene, id: u64) -> &'a super::PhobosTrajectoryState {
    scene
        .sim
        .projectiles
        .get(id)
        .expect("bullet")
        .phobos_trajectory()
        .expect("trajectory state")
}

fn magnitude(velocity: ProjectileVelocity) -> f64 {
    f64_of(velocity.x)
        .hypot(f64_of(velocity.y))
        .hypot(f64_of(velocity.z))
}

fn target_position(scene: &Scene, id: u64) -> ProjectileCoord {
    let entity = scene.sim.substrate.entities.get(id).expect("target");
    let coords = crate::sim::movement::ground_pose::object_get_coords(
        entity,
        scene.sim.resolved_terrain.as_ref(),
    );
    ProjectileCoord::new(coords.x, coords.y, coords.z)
}

fn cell_center(cell: (u16, u16)) -> ProjectileCoord {
    ProjectileCoord::new(
        i32::from(cell.0) * 256 + 128,
        i32::from(cell.1) * 256 + 128,
        0,
    )
}

fn velocity_of(scene: &Scene, id: u64) -> ProjectileVelocity {
    scene.sim.projectiles.get(id).expect("bullet").velocity()
}

fn f64_of(bits: NativeF64Bits) -> f64 {
    f64::from_bits(bits.bits())
}

/// `PhobosTrajectory.cpp:502..531`: a trajectory bullet advances its
/// `LaserTrail.Types` list after `OnAI`, from the current Location to
/// Location + Velocity.
#[test]
fn trajectory_bullets_advance_their_laser_trails() {
    let mut scene = fixture_with_trails();
    let origin = cell_center((22, 10));
    let id = fire(
        &mut scene,
        ProjectileTarget::Cell { rx: 28, ry: 10 },
        origin,
        cell_center((28, 10)),
    );
    Simulation::construct_bullet_scheme(
        &scene.sim.interner,
        &scene.sim.substrate,
        &scene.sim.houses,
        &mut scene.sim.projectiles,
        id,
        &scene.rules,
    );
    let step = scene.sim.step_phobos_trajectory(id, &scene.rules, None);
    assert!(step.detonations.is_empty());
    scene.sim.update_trajectory_laser_trails(id, &scene.rules);
    let births: Vec<_> = scene
        .sim
        .lifecycle_outputs
        .as_slice()
        .iter()
        .filter_map(|output| match output {
            LifecycleOutput::LaserCreated(birth) => Some(*birth),
            _ => None,
        })
        .collect();
    let position = scene.sim.projectiles.get(id).expect("bullet").position();
    assert_eq!(births.len(), 1, "the first step seeds and draws");
    assert_eq!(
        (births[0].from.x, births[0].to.x),
        (position.x, position.x + 16)
    );
    assert_eq!(births[0].duration, 3);
}

#[test]
fn straight_flies_at_trajectory_speed_and_clamps_it() {
    let mut scene = fixture("Trajectory=Straight\nTrajectory.Speed=16\n");
    let origin = cell_center((22, 10));
    let id = fire(
        &mut scene,
        ProjectileTarget::Cell { rx: 28, ry: 10 },
        origin,
        cell_center((28, 10)),
    );
    for _ in 0..3 {
        let step = scene.sim.step_phobos_trajectory(id, &scene.rules, None);
        assert!(step.detonations.is_empty(), "still far from the target");
    }
    let position = scene.sim.projectiles.get(id).expect("bullet").position();
    assert_eq!(position.x - origin.x, 3 * 16);
    assert_eq!(position.y, origin.y);

    // `Math::max(0.001, x)` keeps a zero or negative speed moving.
    for speed in ["0", "-4"] {
        let mut scene = fixture(&format!("Trajectory=Straight\nTrajectory.Speed={speed}\n"));
        let id = fire(
            &mut scene,
            ProjectileTarget::Cell { rx: 28, ry: 10 },
            origin,
            cell_center((28, 10)),
        );
        let velocity = velocity_of(&scene, id);
        assert!(
            (f64_of(velocity.x) - 0.001).abs() < 1e-12,
            "speed {speed} clamps to 0.001"
        );
    }
}

#[test]
fn detonation_distance_detonates_and_snaps_to_the_target() {
    let mut scene = fixture(
        "Trajectory=Straight\nTrajectory.Speed=64\n\
         Trajectory.Straight.DetonationDistance=0.4\n\
         Trajectory.Straight.TargetSnapDistance=0.5\n",
    );
    let target = scene
        .sim
        .spawn_object("TGT", "Americans", 26, 10, 0, &scene.rules)
        .expect("target");
    let origin = cell_center((24, 10));
    let id = fire(
        &mut scene,
        ProjectileTarget::Entity(target),
        origin,
        cell_center((26, 10)),
    );
    let target_coords = crate::sim::movement::ground_pose::object_get_coords(
        scene.sim.substrate.entities.get(target).expect("target"),
        scene.sim.resolved_terrain.as_ref(),
    );
    let mut detonation = None;
    for _ in 0..12 {
        let step = scene.sim.step_phobos_trajectory(id, &scene.rules, None);
        if let Some(detonation_) = step.detonations.first().copied() {
            detonation = Some(detonation_);
            break;
        }
    }
    let detonation = detonation.expect("the bullet detonates inside 0.4 cells");
    assert_eq!(
        detonation.reason,
        ProjectileDetonationReason::PhobosTrajectory
    );
    assert_eq!(
        (
            detonation.impact.x,
            detonation.impact.y,
            detonation.impact.z
        ),
        (target_coords.x, target_coords.y, target_coords.z),
        "TargetSnapDistance snaps the impact onto the target"
    );
    assert!(
        state_of(&scene, id).snapped_to_target,
        "the retained BulletExt::SnappedToTarget byte"
    );
}

#[test]
fn pass_detonate_hits_with_its_warhead_and_damage_on_the_delay() {
    let mut scene = fixture(
        "Trajectory=Straight\nTrajectory.Speed=16\n\
         Trajectory.Straight.PassThrough=yes\n\
         Trajectory.Straight.DetonationDistance=0\n\
         Trajectory.Straight.PassDetonate=yes\n\
         Trajectory.Straight.PassDetonateWarhead=PSST\n\
         Trajectory.Straight.PassDetonateDamage=7\n\
         Trajectory.Straight.PassDetonateDelay=1\n\
         Trajectory.Straight.PassDetonateInitialDelay=0\n",
    );
    let id = fire(
        &mut scene,
        ProjectileTarget::Cell { rx: 28, ry: 10 },
        cell_center((22, 10)),
        cell_center((28, 10)),
    );
    let psst = scene.sim.interner.intern("PSST");
    let step = scene.sim.step_phobos_trajectory(id, &scene.rules, None);
    let detonation = step
        .detonations
        .iter()
        .chain(&step.extra_detonations)
        .find(|detonation| detonation.payload.warhead == psst)
        .expect("a pass-through detonation on the first frame");
    assert_eq!(detonation.payload.base_damage, 7);
    assert_eq!(detonation.target, ProjectileTarget::None);

    // The timer restarted: the next frame is quiet, the one after hits again.
    let quiet = scene.sim.step_phobos_trajectory(id, &scene.rules, None);
    assert!(
        !quiet
            .detonations
            .iter()
            .chain(&quiet.extra_detonations)
            .any(|detonation| detonation.payload.warhead == psst),
        "the delay spaces the pass hits"
    );
}

#[test]
fn proximity_hits_an_enemy_along_the_flight_path() {
    let mut scene = fixture(
        "Trajectory=Straight\nTrajectory.Speed=64\n\
         Trajectory.Straight.PassThrough=yes\n\
         Trajectory.Straight.DetonationDistance=0\n\
         Trajectory.Straight.ProximityImpact=1\n\
         Trajectory.Straight.ProximityWarhead=PSST\n\
         Trajectory.Straight.ProximityDamage=5\n\
         Trajectory.Straight.ProximityRadius=1\n",
    );
    let target = scene
        .sim
        .spawn_object("TGT", "Americans", 26, 10, 0, &scene.rules)
        .expect("target");
    let id = fire(
        &mut scene,
        ProjectileTarget::Cell { rx: 28, ry: 10 },
        cell_center((22, 10)),
        cell_center((28, 10)),
    );
    let psst = scene.sim.interner.intern("PSST");
    let mut hit = None;
    for _ in 0..60 {
        let step = scene.sim.step_phobos_trajectory(id, &scene.rules, None);
        if let Some(detonation) = step
            .detonations
            .iter()
            .chain(&step.extra_detonations)
            .find(|detonation| detonation.payload.warhead == psst)
        {
            hit = Some(*detonation);
            break;
        }
    }
    let hit = hit.expect("the proximity radius catches the target");
    assert_eq!(hit.target, ProjectileTarget::Entity(target));
    assert_eq!(hit.payload.base_damage, 5);
}

#[test]
fn through_vehicles_off_detonates_on_the_obstacle() {
    let proj_keys = |through: &str| {
        format!(
            "Trajectory=Straight\nTrajectory.Speed=64\n\
             Trajectory.Straight.PassThrough=yes\n\
             Trajectory.Straight.DetonationDistance=0\n\
             Trajectory.Straight.ThroughVehicles={through}\n"
        )
    };
    for (through, expect_hit) in [("no", true), ("yes", false)] {
        let mut scene = fixture(&proj_keys(through));
        let blocker = scene
            .sim
            .spawn_object("BLOCK", "Americans", 25, 10, 0, &scene.rules)
            .expect("blocker");
        let id = fire(
            &mut scene,
            ProjectileTarget::Cell { rx: 28, ry: 10 },
            cell_center((22, 10)),
            cell_center((28, 10)),
        );
        let mut hit = false;
        for _ in 0..40 {
            let step = scene.sim.step_phobos_trajectory(id, &scene.rules, None);
            if let Some(detonation) = step.detonations.first() {
                hit = detonation.target == ProjectileTarget::Entity(blocker)
                    || detonation.target == ProjectileTarget::Cell { rx: 28, ry: 10 };
                break;
            }
        }
        assert_eq!(
            hit, expect_hit,
            "ThroughVehicles={through}: the vehicle blocks or not"
        );
    }
}

#[test]
fn subject_to_ground_detonates_below_the_floor_and_confine_lifts() {
    let mut scene = fixture(
        "Trajectory=Straight\nTrajectory.Speed=16\n\
         Trajectory.Straight.SubjectToGround=yes\n",
    );
    let below = ProjectileCoord::new(22 * 256 + 128, 10 * 256 + 128, -100);
    let id = fire(
        &mut scene,
        ProjectileTarget::Cell { rx: 28, ry: 10 },
        below,
        cell_center((28, 10)),
    );
    let step = scene.sim.step_phobos_trajectory(id, &scene.rules, None);
    assert_eq!(
        step.detonations.len(),
        1,
        "a bullet below the cell floor detonates"
    );

    let mut scene = fixture(
        "Trajectory=Straight\nTrajectory.Speed=16\n\
         Trajectory.Straight.ConfineAtHeight=100\n",
    );
    let id = fire(
        &mut scene,
        ProjectileTarget::Cell { rx: 28, ry: 10 },
        cell_center((22, 10)),
        cell_center((28, 10)),
    );
    let _ = scene.sim.step_phobos_trajectory(id, &scene.rules, None);
    assert!(
        f64_of(velocity_of(&scene, id).z) > 0.0,
        "ConfineAtHeight raises the flight"
    );
}

#[test]
fn bombard_defaults_throw_from_the_height_and_speed() {
    let mut scene = fixture(
        "Trajectory=Bombard\nTrajectory.Speed=32\n\
         Trajectory.Bombard.Height=200\n\
         Trajectory.Bombard.FallSpeed=16\n",
    );
    let origin = cell_center((22, 10));
    let id = fire(
        &mut scene,
        ProjectileTarget::Cell { rx: 28, ry: 10 },
        origin,
        cell_center((28, 10)),
    );
    let bullet = scene.sim.projectiles.get(id).expect("bullet");
    assert_eq!(bullet.position().z, 200, "the middle sits at Height");
    assert!((magnitude(bullet.velocity()) - 32.0).abs() < 1e-6);
    let state = state_of(&scene, id);
    assert_eq!(state.kind, TrajectoryFlag::Bombard);
    assert!((f64_of(state.bombard_fall_percent) - 1.0).abs() < 1e-12);
    assert!(!state.is_falling);
}

#[test]
fn bombard_cruises_then_falls_and_detonates_on_the_target() {
    let mut scene = fixture(
        "Trajectory=Bombard\nTrajectory.Speed=32\n\
         Trajectory.Bombard.Height=100\n\
         Trajectory.Bombard.FallSpeed=32\n\
         Trajectory.Bombard.DetonationDistance=0.5\n",
    );
    let target = cell_center((28, 10));
    let id = fire(
        &mut scene,
        ProjectileTarget::Cell { rx: 28, ry: 10 },
        cell_center((22, 10)),
        target,
    );
    let mut detonation = None;
    for _ in 0..200 {
        let step = scene.sim.step_phobos_trajectory(id, &scene.rules, None);
        if let Some(hit) = step.detonations.first().copied() {
            detonation = Some(hit);
            break;
        }
    }
    let detonation = detonation.expect("the bomb reaches the target");
    assert_eq!(
        detonation.reason,
        ProjectileDetonationReason::PhobosTrajectory
    );
    let distance_x = detonation.impact.x - target.x;
    let distance_y = detonation.impact.y - target.y;
    assert!(
        (distance_x * distance_x + distance_y * distance_y) < 256 * 256,
        "DetonationDistance ends the fall"
    );
}

#[test]
fn bombard_early_detonation_checks_the_source_height() {
    let mut scene = fixture(
        "Trajectory=Bombard\nTrajectory.Speed=16\n\
         Trajectory.Bombard.Height=300\n\
         Trajectory.Bombard.DetonationHeight=50\n\
         Trajectory.Bombard.EarlyDetonation=yes\n",
    );
    let id = fire(
        &mut scene,
        ProjectileTarget::Cell { rx: 28, ry: 10 },
        cell_center((22, 10)),
        cell_center((28, 10)),
    );
    let step = scene.sim.step_phobos_trajectory(id, &scene.rules, None);
    assert_eq!(step.detonations.len(), 1, "the middle is above 50 leptons");
}

#[test]
fn bombard_fall_scatter_and_offset_shift_the_middle() {
    let mut scene = fixture(
        "Trajectory=Bombard\nTrajectory.Speed=16\n\
         Trajectory.Bombard.Height=100\n\
         Trajectory.Bombard.FallScatter.Min=1\n\
         Trajectory.Bombard.FallScatter.Max=1\n",
    );
    let target = cell_center((28, 10));
    let id = fire(
        &mut scene,
        ProjectileTarget::Cell { rx: 28, ry: 10 },
        cell_center((22, 10)),
        target,
    );
    let middle = scene.sim.projectiles.get(id).expect("bullet").position();
    let scatter = f64::from(middle.x - target.x).hypot(f64::from(middle.y - target.y));
    assert!((scatter - 256.0).abs() < 1.0, "a one-cell radial scatter");

    let mut scene = fixture(
        "Trajectory=Bombard\nTrajectory.Speed=16\n\
         Trajectory.Bombard.Height=100\n\
         Trajectory.Bombard.OffsetCoord=100,0\n",
    );
    let id = fire(
        &mut scene,
        ProjectileTarget::Cell { rx: 28, ry: 10 },
        cell_center((22, 10)),
        target,
    );
    assert_eq!(state_of(&scene, id).last_target_coord.x, target.x + 100);
}

#[test]
fn bombard_no_launch_starts_falling_at_once() {
    let mut scene = fixture(
        "Trajectory=Bombard\nTrajectory.Speed=16\n\
         Trajectory.Bombard.Height=200\n\
         Trajectory.Bombard.FallSpeed=16\n\
         Trajectory.Bombard.NoLaunch=yes\n\
         Trajectory.Bombard.FreeFallOnTarget=no\n",
    );
    let id = fire(
        &mut scene,
        ProjectileTarget::Cell { rx: 28, ry: 10 },
        cell_center((22, 10)),
        cell_center((28, 10)),
    );
    let state = state_of(&scene, id);
    assert!(state.is_falling);
    let bullet = scene.sim.projectiles.get(id).expect("bullet");
    assert_eq!(bullet.position().z, 200);
    assert!((f64_of(bullet.velocity().z) + 16.0).abs() < 1e-6);
}

#[test]
fn bombard_lead_waits_one_frame_and_predicts_a_moving_target() {
    let mut scene = fixture(
        "Trajectory=Bombard\nTrajectory.Speed=32\n\
         Trajectory.Bombard.Height=200\n\
         Trajectory.Bombard.NoLaunch=yes\n\
         Trajectory.Bombard.LeadTimeCalculate=yes\n",
    );
    let target_id = scene
        .sim
        .spawn_object("TGT", "Americans", 26, 10, 0, &scene.rules)
        .expect("target");
    let initial = target_position(&scene, target_id);
    let id = fire(
        &mut scene,
        ProjectileTarget::Entity(target_id),
        cell_center((22, 10)),
        initial,
    );
    assert_eq!(state_of(&scene, id).wait_one_frame, 2);

    scene
        .sim
        .substrate
        .entities
        .get_mut(target_id)
        .expect("target")
        .position
        .set_rx_for_test(27);
    let _ = scene.sim.step_phobos_trajectory(id, &scene.rules, None);
    assert_eq!(state_of(&scene, id).wait_one_frame, 1);

    scene
        .sim
        .substrate
        .entities
        .get_mut(target_id)
        .expect("target")
        .position
        .set_rx_for_test(28);
    let _ = scene.sim.step_phobos_trajectory(id, &scene.rules, None);
    let live = target_position(&scene, target_id);
    assert!(
        state_of(&scene, id).last_target_coord.x > live.x,
        "the lead solve predicts the next target cell"
    );
}

#[test]
fn bombard_disperse_burst_rotates_the_burst() {
    let mut scene = fixture_burst(
        "Trajectory=Bombard\nTrajectory.Speed=32\n\
         Trajectory.Bombard.Height=200\n\
         Trajectory.Bombard.RotateCoord=90\n\
         Trajectory.Bombard.MirrorCoord=no\n",
        3,
    );
    let id = fire(
        &mut scene,
        ProjectileTarget::Cell { rx: 28, ry: 10 },
        cell_center((22, 10)),
        cell_center((28, 10)),
    );
    assert!(
        f64_of(scene.sim.projectiles.get(id).expect("bullet").velocity().y).abs() > 1.0,
        "a non-mirrored burst rotates the launch vector"
    );
}

#[test]
fn parabola_default_speed_mode_moves_horizontally_at_trajectory_speed() {
    let mut scene = fixture("Trajectory=Parabola\nTrajectory.Speed=32\n");
    let id = fire(
        &mut scene,
        ProjectileTarget::Cell { rx: 26, ry: 10 },
        cell_center((22, 10)),
        cell_center((26, 10)),
    );
    let velocity = scene.sim.projectiles.get(id).expect("bullet").velocity();
    let horizontal = f64_of(velocity.x).hypot(f64_of(velocity.y));
    assert!((horizontal - 32.0).abs() < 1e-6);
    assert!(f64_of(velocity.z) > 0.0, "Speed mode lobs to the target");
    assert_eq!(state_of(&scene, id).throw_height, 600);
}

#[test]
fn parabola_height_mode_throws_above_the_target_and_detonates() {
    let mut scene = fixture(
        "Trajectory=Parabola\nTrajectory.Speed=32\n\
         Trajectory.Parabola.OpenFireMode=height\n\
         Trajectory.Parabola.ThrowHeight=100\n",
    );
    let id = fire(
        &mut scene,
        ProjectileTarget::Cell { rx: 26, ry: 10 },
        cell_center((22, 10)),
        cell_center((26, 10)),
    );
    assert!(
        f64_of(scene.sim.projectiles.get(id).expect("bullet").velocity().z) > 20.0,
        "the throw rises to about ThrowHeight"
    );
    let mut detonated = false;
    for _ in 0..200 {
        let step = scene.sim.step_phobos_trajectory(id, &scene.rules, None);
        if !step.detonations.is_empty() {
            detonated = true;
            break;
        }
    }
    assert!(detonated, "the arc lands and detonates");
}

#[test]
fn parabola_detonation_angle_ends_the_descent() {
    let mut scene = fixture(
        "Trajectory=Parabola\nTrajectory.Speed=32\n\
         Trajectory.Parabola.DetonationAngle=45\n",
    );
    let id = fire(
        &mut scene,
        ProjectileTarget::Cell { rx: 26, ry: 10 },
        cell_center((22, 10)),
        cell_center((26, 10)),
    );
    let mut detonated = false;
    for _ in 0..200 {
        let step = scene.sim.step_phobos_trajectory(id, &scene.rules, None);
        if !step.detonations.is_empty() {
            detonated = true;
            break;
        }
    }
    assert!(detonated, "the flight ends on the descent angle");
}

#[test]
fn parabola_offset_coord_shifts_the_aim() {
    let mut scene = fixture(
        "Trajectory=Parabola\nTrajectory.Speed=32\n\
         Trajectory.Parabola.OffsetCoord=100,0\n",
    );
    let target = cell_center((26, 10));
    let id = fire(
        &mut scene,
        ProjectileTarget::Cell { rx: 26, ry: 10 },
        cell_center((22, 10)),
        target,
    );
    assert_eq!(state_of(&scene, id).last_target_coord.x, target.x + 100);
}

#[test]
fn parabola_bounce_detonates_once_and_attenuates() {
    let mut scene = fixture(
        "Trajectory=Parabola\nTrajectory.Speed=32\n\
         Trajectory.Parabola.OpenFireMode=height\n\
         Trajectory.Parabola.ThrowHeight=100\n\
         Trajectory.Parabola.DetonationDistance=-1\n\
         Trajectory.Parabola.BounceTimes=1\n\
         Trajectory.Parabola.BounceDetonate=yes\n\
         Trajectory.Parabola.BounceAttenuation=0.5\n",
    );
    let id = fire(
        &mut scene,
        ProjectileTarget::Cell { rx: 26, ry: 10 },
        cell_center((22, 10)),
        cell_center((26, 10)),
    );
    let mut bounced = false;
    let mut detonated = false;
    for _ in 0..200 {
        let step = scene.sim.step_phobos_trajectory(id, &scene.rules, None);
        if !step.extra_detonations.is_empty() {
            bounced = true;
            assert_eq!(step.extra_detonations[0].payload.base_damage, 10);
        }
        if !step.detonations.is_empty() {
            detonated = true;
            break;
        }
    }
    assert!(bounced, "BounceDetonate fires on the bounce");
    assert!(detonated, "the second contact detonates");
    assert_eq!(state_of(&scene, id).bounce_times, 0);
    assert_eq!(
        scene
            .sim
            .projectiles
            .get(id)
            .expect("bullet")
            .payload()
            .base_damage,
        5,
        "BounceAttenuation scales the remaining damage"
    );
}

#[test]
fn parabola_water_without_bounce_on_water_detonates() {
    let mut scene = fixture(
        "Trajectory=Parabola\nTrajectory.Speed=32\n\
         Trajectory.Parabola.OpenFireMode=height\n\
         Trajectory.Parabola.ThrowHeight=100\n\
         Trajectory.Parabola.DetonationDistance=-1\n\
         Trajectory.Parabola.BounceTimes=1\n\
         Trajectory.Parabola.BounceDetonate=yes\n",
    );
    crate::sim::arena_fixture::flood(
        &mut scene.sim,
        &scene.rules,
        32,
        [(25, 10), (26, 10), (27, 10)],
    );
    let id = fire(
        &mut scene,
        ProjectileTarget::Cell { rx: 26, ry: 10 },
        cell_center((22, 10)),
        cell_center((26, 10)),
    );
    let mut bounced = false;
    let mut detonated = false;
    for _ in 0..200 {
        let step = scene.sim.step_phobos_trajectory(id, &scene.rules, None);
        bounced |= !step.extra_detonations.is_empty();
        if !step.detonations.is_empty() {
            detonated = true;
            break;
        }
    }
    assert!(detonated, "water without BounceOnWater ends the flight");
    assert!(!bounced, "no bounce detonation on water");
    assert_eq!(state_of(&scene, id).bounce_times, 1);
}

#[test]
fn parabola_lead_waits_one_frame_and_predicts_a_moving_target() {
    let mut scene = fixture(
        "Trajectory=Parabola\nTrajectory.Speed=300\n\
         Trajectory.Parabola.LeadTimeCalculate=yes\n",
    );
    let target_id = scene
        .sim
        .spawn_object("TGT", "Americans", 26, 10, 0, &scene.rules)
        .expect("target");
    let initial = target_position(&scene, target_id);
    let id = fire(
        &mut scene,
        ProjectileTarget::Entity(target_id),
        cell_center((22, 10)),
        initial,
    );
    assert_eq!(state_of(&scene, id).wait_one_frame, 2);

    scene
        .sim
        .substrate
        .entities
        .get_mut(target_id)
        .expect("target")
        .position
        .set_rx_for_test(27);
    let _ = scene.sim.step_phobos_trajectory(id, &scene.rules, None);
    assert_eq!(state_of(&scene, id).wait_one_frame, 1);

    scene
        .sim
        .substrate
        .entities
        .get_mut(target_id)
        .expect("target")
        .position
        .set_rx_for_test(28);
    let _ = scene.sim.step_phobos_trajectory(id, &scene.rules, None);
    let live = target_position(&scene, target_id);
    assert!(
        state_of(&scene, id).last_target_coord.x > live.x,
        "the lead solve aims ahead of the target"
    );
}

#[test]
fn parabola_disperse_burst_rotates_the_burst() {
    let mut scene = fixture_burst(
        "Trajectory=Parabola\nTrajectory.Speed=32\n\
         Trajectory.Parabola.RotateCoord=90\n\
         Trajectory.Parabola.MirrorCoord=no\n",
        3,
    );
    let id = fire(
        &mut scene,
        ProjectileTarget::Cell { rx: 26, ry: 10 },
        cell_center((22, 10)),
        cell_center((26, 10)),
    );
    assert!(
        f64_of(scene.sim.projectiles.get(id).expect("bullet").velocity().y).abs() > 1.0,
        "a non-mirrored burst rotates the launch vector"
    );
}

#[test]
fn parabola_without_the_flag_keeps_the_stock_arms() {
    let mut scene = fixture(
        "Trajectory.Parabola.ThrowHeight=100\n\
         Trajectory.Parabola.BounceTimes=2\n",
    );
    let id = fire(
        &mut scene,
        ProjectileTarget::Cell { rx: 28, ry: 10 },
        cell_center((22, 10)),
        cell_center((28, 10)),
    );
    assert!(
        !scene
            .sim
            .projectiles
            .get(id)
            .expect("bullet")
            .has_phobos_trajectory()
    );
    assert!(
        scene
            .rules
            .projectile("PROJ")
            .expect("type")
            .phobos_trajectory
            .is_stock()
    );
}

#[test]
fn a_bullet_without_the_trajectory_keeps_the_stock_arms() {
    let mut scene = fixture("");
    let id = fire(
        &mut scene,
        ProjectileTarget::Cell { rx: 28, ry: 10 },
        cell_center((22, 10)),
        cell_center((28, 10)),
    );
    assert!(
        !scene
            .sim
            .projectiles
            .get(id)
            .expect("bullet")
            .has_phobos_trajectory()
    );
    assert!(
        scene
            .rules
            .projectile("PROJ")
            .expect("type")
            .phobos_trajectory
            .is_stock()
    );
}

/// `ParabolaTrajectory::OnUnlimbo`'s ground reset (`:131-139`) repoints the
/// bullet's Target at the foot's cell and sets its TargetCoords, but leaves
/// `LastTargetCoord` at the pre-reset aim, so a later lead solve still
/// predicts from it.
#[test]
fn a_ground_reset_keeps_the_last_target_coord() {
    let mut scene = fixture(
        "Trajectory=Parabola\nTrajectory.Speed=32\n\
         Trajectory.Parabola.DetonationDistance=-1\n",
    );
    let target = scene
        .sim
        .spawn_object("TGT", "Americans", 26, 10, 0, &scene.rules)
        .expect("target");
    let id = fire(
        &mut scene,
        ProjectileTarget::Entity(target),
        cell_center((22, 10)),
        cell_center((22, 10)),
    );
    let projectile = scene.sim.projectiles.get(id).expect("bullet");
    assert!(
        matches!(
            projectile.target(),
            ProjectileTarget::Cell { rx: 26, ry: 10 }
        ),
        "the reset repoints the target at the foot's cell"
    );
    assert_eq!(
        projectile.last_target_position(),
        cell_center((22, 10)),
        "LastTargetCoord keeps the pre-reset aim"
    );
    assert_eq!(
        state_of(&scene, id).last_target_coord,
        crate::sim::projectile::cell_ground_coord(scene.sim.resolved_terrain.as_ref(), 26, 10),
        "the mutable TargetCoords becomes the cell's ground coordinate"
    );
}
