//! The decidable `Fire` (RFC 0001 section 9 "Decidable `Fire`", appendix
//! 4.3, tier preview): a module answers a shot plan, a list of
//! `ProjectileSpec`s, and FireAt launches those bullets in place of its own.
//!
//! Decision point: `TechnoClass::FireAt @ 0x006FDD50` once its launch math
//! has a solution (`fireat_launch`, `0x006FE663..0x006FEA52`) and its bullet
//! payload is built, just before `BulletClass::Fire @ 0x006FF014`
//! (`emit_admitted_fire`). Every Scenario draw of the shot (scatter, the
//! radial index, the bullet id) has happened by then, so a plan moves no
//! random stream. Not asked: a shot without a launch solution (it fails, and
//! the tail runs as for any failed launch), the arms that make no bullet
//! (`Suicide=`, Sonic, Spawner, Drain, DiskLaser) and `LimboLaunch=`
//! weapons, whose bullet carries the firer (Parasite).
//!
//! The first link gets the native shot as one entry: its projectile type,
//! weapon −1, target 0, data 0, offset 0. An unanswered chain fires
//! natively. An answered plan launches each valid entry, in order:
//! - `projectile_type`: a BulletType ID; its own flight arm, guidance and
//!   collision apply ([`super::projectile_delivery`]).
//! - `weapon`: −1 is the shot's own weapon and payload (damage after every
//!   firing multiplier, a Prism support bonus, Ares range); another ID fires
//!   that weapon's `Damage=`, warhead and `Bright=`.
//! - `target`: 0 is the shot's target; an object or cell reference retargets.
//! - `origin`: an offset from the native launch position, or absolute with
//!   flag bit 0.
//! - `data`: the bullet's `Data0`/`Data1` from launch.
//!
//! An entry equal to the native one (same type, target 0, no offset) flies
//! exactly as the native bullet: same velocity and speed (with another weapon
//! only the payload differs). Any other entry takes FireAt's scalar launch
//! (`fireat_launch`) from its origin to its target's coordinate at its
//! weapon's `GetSpeed` (`weapon_launch_speed`), without the radial fan, the
//! lead, the launch scatter, the Lobber high root or the building pitch; an
//! entry without a ballistic solution is skipped. The first entry takes the
//! shot's bullet id, later entries new ones. Entries naming no BulletType,
//! no weapon with a warhead, or a gone object are skipped.
//!
//! The rest of the shot is unchanged, a plan or none (RFC 9): the occupant
//! advance, recoil, particle systems, railgun line damage and laser, bolt,
//! beam and wave effects (still toward the native target), burst, ROF and
//! rearm, report, muzzle anim, ammo, RevealOnFire and the FireAt tail. The
//! shot's bullet for the rest of FireAt (the Unit Scalable enrolment, an
//! aircraft's drop velocity) is the plan's first launched bullet, or none.
//! The delivered `Fire` has `projectile` 0; `AttackedBy` is raised once per
//! launched bullet whose target is an object.

use vera20k_wasm_ext::decide::ProjectileSpec;
use vera20k_wasm_ext::triggers::EVENT_FIRE;

use super::projectile_delivery::classify_projectile_type;
use super::{FiredBullet, TargetKind};
use crate::rules::ruleset::RuleSet;
use crate::rules::weapon_type::WeaponType;
use crate::sim::ext_modules::ModuleEvent;
use crate::sim::ext_modules::projectiles::{SpecTarget, spec_target};
use crate::sim::projectile::launch::{
    FireAtLaunch, LaunchSpeedProjectile, fireat_launch, fireat_launch_distance,
    weapon_launch_speed,
};
use crate::sim::projectile::{ProjectileCoord, ProjectileTarget, ProjectileVelocity};
use crate::sim::world::Simulation;

/// The native shot a `Fire` decision is about.
pub(super) struct NativeShot<'a> {
    pub(super) shooter: u64,
    pub(super) target: TargetKind,
    pub(super) target_cell: (u16, u16),
    /// The weapon slot.
    pub(super) slot: i64,
    pub(super) weapon: &'a WeaponType,
    pub(super) weapon_name: &'a str,
    /// The id `CreateBullet` took for the native bullet.
    pub(super) bullet_id: u64,
    pub(super) bullet: FiredBullet,
}

/// One launched plan bullet and its target object (0 for a cell).
pub(super) type PlanBullet = (u64, u64);

/// Asks `Fire` for `shot` (module docs); `None` fires natively.
pub(super) fn decide_fire_plan(
    world: &mut Simulation,
    rules: &RuleSet,
    shot: &NativeShot<'_>,
) -> Option<Vec<ProjectileSpec>> {
    if !world.trigger_runtime.module_decides(EVENT_FIRE) || shot.weapon.limbo_launch {
        return None;
    }
    let index = |found: Option<u32>| found.map_or(-1, |index| index as i32);
    let weapon = index(rules.weapon_index(shot.weapon_name));
    let warhead = index(rules.warhead_index(world.interner.resolve(shot.bullet.payload.warhead)));
    let projectile_type = index(
        shot.bullet
            .payload
            .projectile_type()
            .and_then(|id| rules.projectile_index(world.interner.resolve(id))),
    );
    let native = ProjectileSpec {
        projectile_type,
        weapon: -1,
        ..ProjectileSpec::default()
    };
    let event = ModuleEvent {
        other: match shot.target {
            TargetKind::Entity(id) => id,
            TargetKind::Cell(..) => 0,
        },
        cell: Some(shot.target_cell),
        value: shot.slot,
        id: weapon,
        weapon,
        warhead,
        projectile_type,
        ..ModuleEvent::new(EVENT_FIRE, shot.shooter)
    };
    let mut area = native.to_bytes().to_vec();
    let count = world.decide_module_area(rules, &event, 1, &mut area)?;
    Some(
        (0..usize::try_from(count).ok()?)
            .filter_map(|i| ProjectileSpec::from_bytes(area.get(i * ProjectileSpec::SIZE..)?))
            .collect(),
    )
}

/// Launches `plan` in place of `shot`'s bullet (module docs), in order.
pub(super) fn launch_fire_plan(
    world: &mut Simulation,
    rules: &RuleSet,
    shot: &NativeShot<'_>,
    plan: &[ProjectileSpec],
) -> Vec<PlanBullet> {
    let native_type = shot.bullet.payload.projectile_type();
    let mut launched = Vec::with_capacity(plan.len());
    for spec in plan {
        let Some(kind) = u32::try_from(spec.projectile_type)
            .ok()
            .and_then(|id| rules.projectile_at(id))
        else {
            continue;
        };
        let type_id = world.interner.intern(&kind.id);
        let (weapon, payload) = match spec.weapon {
            -1 => (
                shot.weapon,
                shot.bullet.payload.with_projectile_type(Some(type_id)),
            ),
            id => {
                let Some(weapon) = u32::try_from(id).ok().and_then(|id| rules.weapon_at(id))
                else {
                    continue;
                };
                let Some(warhead) = rules.weapon_warhead(weapon) else {
                    continue;
                };
                let payload = crate::sim::projectile::ProjectilePayload::new(
                    weapon.damage,
                    world.interner.intern(&warhead.id),
                    Some(type_id),
                    weapon.bright,
                )
                .with_installed_weapon(Some(world.interner.intern(&weapon.id)));
                (weapon, payload)
            }
        };
        let target = match spec_target(spec.target) {
            None if spec.target == 0 => Some(shot.bullet.target),
            None => None,
            Some(SpecTarget::Object(id)) => world
                .substrate
                .entities
                .get(id)
                .is_some()
                .then_some(ProjectileTarget::Entity(id)),
            Some(SpecTarget::Cell(x, y)) => Some(ProjectileTarget::looked_up_cell(
                world.resolved_terrain.as_ref(),
                x,
                y,
            )),
        };
        let Some(target) = target else {
            continue;
        };
        let absolute = spec.flags & ProjectileSpec::ABSOLUTE_ORIGIN != 0;
        let origin = if absolute {
            ProjectileCoord::new(spec.origin[0], spec.origin[1], spec.origin[2])
        } else {
            let native = shot.bullet.origin;
            ProjectileCoord::new(
                native.x.wrapping_add(spec.origin[0]),
                native.y.wrapping_add(spec.origin[1]),
                native.z.wrapping_add(spec.origin[2]),
            )
        };
        let as_native = Some(type_id) == native_type
            && spec.target == 0
            && !absolute
            && spec.origin == [0; 3];
        let bullet = if as_native {
            FiredBullet {
                payload,
                ..shot.bullet
            }
        } else {
            match planned_bullet(world, rules, shot, weapon, kind, target, payload, origin) {
                Some(bullet) => bullet,
                None => continue,
            }
        };
        // The first entry takes the shot's bullet id (`CreateBullet`
        // already spent it); later entries take their own.
        let (bullet_id, bullet) = if launched.is_empty() {
            (shot.bullet_id, bullet)
        } else {
            let id = world.substrate.allocate_stable_id();
            let native_unique_id = world.next_native_runtime_id();
            (
                id,
                FiredBullet {
                    native_unique_id,
                    ..bullet
                },
            )
        };
        world.module_projectile_data(bullet_id, spec.data);
        let admitted =
            super::admit_techno_fired_bullet(world, rules, weapon, bullet_id, bullet);
        if !admitted || world.projectiles.get(bullet_id).is_none() {
            world.forget_module_projectile(bullet_id);
            if !admitted {
                continue;
            }
        }
        let target_object = match target {
            ProjectileTarget::Entity(id) => id,
            _ => 0,
        };
        launched.push((bullet_id, target_object));
    }
    launched
}

/// A plan entry that is not the native bullet: FireAt's scalar launch from
/// `origin` to `target` (module docs). `None` without a target coordinate
/// or a ballistic solution.
#[allow(clippy::too_many_arguments)]
fn planned_bullet(
    world: &mut Simulation,
    rules: &RuleSet,
    shot: &NativeShot<'_>,
    weapon: &WeaponType,
    kind: &crate::rules::projectile_type::ProjectileType,
    target: ProjectileTarget,
    payload: crate::sim::projectile::ProjectilePayload,
    origin: ProjectileCoord,
) -> Option<FiredBullet> {
    // `launched` reads the target coordinate Fire keeps (vt+0x58).
    let aim = FiredBullet::launched(
        world,
        rules,
        shot.bullet.native_unique_id,
        shot.shooter,
        target,
        payload,
        weapon.speed,
        origin,
        ProjectileVelocity::new(0, 0, 0),
    )?
    .target_position;
    let delivery = classify_projectile_type(kind, weapon.speed, rules);
    let speed = weapon_launch_speed(
        weapon.speed,
        Some(LaunchSpeedProjectile {
            rot: kind.rot,
            floater: kind.floater,
            gravity: kind.gravity(),
        }),
        rules.general.gravity,
        fireat_launch_distance(origin, aim, world.session.map_exceeds_original_size()),
    );
    let launch = fireat_launch(FireAtLaunch {
        delta: ProjectileCoord::new(
            aim.x.wrapping_sub(origin.x),
            aim.y.wrapping_sub(origin.y),
            aim.z.wrapping_sub(origin.z),
        ),
        speed,
        vertical: delivery.vertical.is_some(),
        homing: delivery.guidance.is_some(),
        heading: None,
        radial_segments: 0,
        radial_index: 0,
        body_heading: 0,
        arcing: delivery.ballistic,
        gravity: crate::sim::projectile::projectile_gravity(
            rules.general.gravity,
            delivery.collision.gravity(),
            delivery.collision.floater,
        ),
        high_root: false,
        voxel_downward: None,
        building_pitch_height: None,
    })?;
    let mut bullet = FiredBullet::launched(
        world,
        rules,
        shot.bullet.native_unique_id,
        shot.shooter,
        target,
        payload,
        weapon.speed,
        origin,
        launch.velocity,
    )?;
    // FireAt keeps its scalar speed for an unguided bullet.
    if delivery.guidance.is_none() {
        bullet.speed = launch.speed.clamp(0, i32::from(u16::MAX)) as u16;
    }
    Some(bullet)
}
