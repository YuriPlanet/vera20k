//! The BulletType facts a shot carries ([`ProjectileDelivery`]) and the
//! admission of a fired Bullet into the world ([`admit_fired_bullet`]).

use super::*;

/// Fire 468A49 dispatches target WhatAmI (+2C); Aircraft's 41C180
/// returns 2 and selects zero Arm independently of current altitude/layer.
pub(super) fn projectile_arm_delay(
    arm: i32,
    target: ProjectileTarget,
    entities: &EntityStore,
) -> i32 {
    if matches!(target, ProjectileTarget::Entity(id)
        if entities.get(id).is_some_and(|entity| entity.category() == EntityCategory::Aircraft))
    {
        0
    } else {
        arm
    }
}

/// The BulletType facts FireAt and `BulletClass::Fire` read for one shot.
/// Every shot is a bullet; `Inviso=` places it at its target. Detonation
/// still follows the common AI admission, including its Arm-gated proximity
/// fuse (SQDJUMP has Arm=2); `fire_inviso` does not bypass that timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ProjectileDelivery {
    arm_frames: i32,
    tracks_target: bool,
    pub(super) collision: ProjectileCollisionPolicy,
    pub(super) ballistic: bool,
    /// `Vertical=` (`BulletTypeClass+0x2C0`) selects the third
    /// `BulletClass::AI` arm. Carries `DetonationAltitude=` (`+0x2BC`).
    pub(super) vertical: Option<i32>,
    /// `BulletTypeClass::Acceleration` (`+0x2D0`), constructor default 3.
    acceleration: i32,
    /// `Inaccurate= && Arcing=` — the launch-time scatter gate at
    /// `TechnoClass::FireAt 0x006FE67D`/`0x006FE68B`. `Some(true)` takes
    /// the range-scaled flak arm, `Some(false)` the plain arm.
    pub(super) launch_scatter_is_flak: Option<bool>,
    pub(super) guidance: Option<ProjectileGuidance>,
    /// `Inviso=` (`+0x29E`): `BulletClass::Fire` places the bullet on its
    /// target with no speed (`0x004688B7..0x00468A39`).
    pub(super) inviso: bool,
}

/// VERA-internal: a weapon naming no BulletType, or one that does not
/// resolve, never occurs in retail rules (native would dereference NULL at
/// `0x006FE55D`). Such a shot is fired as a default BulletType with
/// `Inviso=yes`, so fixtures land in the firing frame like the small-arms shots
/// they stand for; its detonation takes no scatter or cluster draws.
fn missing_projectile_fallback() -> &'static crate::rules::projectile_type::ProjectileType {
    static FALLBACK: std::sync::OnceLock<crate::rules::projectile_type::ProjectileType> =
        std::sync::OnceLock::new();
    FALLBACK.get_or_init(|| {
        let ini = crate::rules::ini_parser::IniFile::from_str("[MissingBulletType]\nInviso=yes\n");
        crate::rules::projectile_type::ProjectileType::from_ini_section(
            "MissingBulletType",
            ini.section("MissingBulletType").expect("fallback section"),
            None,
        )
    })
}

/// The BulletType a shot of `weapon` carries: its `Projectile=`, or
/// [`missing_projectile_fallback`] for one naming none.
pub(super) fn bullet_type<'r>(
    weapon: &crate::rules::weapon_type::WeaponType,
    rules: &'r RuleSet,
) -> &'r crate::rules::projectile_type::ProjectileType {
    weapon
        .projectile
        .as_deref()
        .and_then(|projectile_id| rules.projectile(projectile_id))
        .unwrap_or_else(|| missing_projectile_fallback())
}

/// The BulletType facts a weapon's shot is fired with, and its flight arm.
///
/// gamemd-derived: `BulletClass::AI @ 0x004666E0` has exactly two branches,
/// keyed on `ROT < 1`; the non-homing arm then splits on `Vertical` (`+0x2C0`).
/// `Arcing`, `SubjectToCliffs`, `SubjectToElevation`, `SubjectToWalls`,
/// `Proximity`, `FlakScatter`, `Inviso` and `Cluster` never select an arm —
/// they are launch-time, collision-probe or detonation-time keys — so only
/// `ROT` and `Vertical` pick a flight model.
///
/// Evidence-backed exclusions (verified 2026-09-03, exhaustive over the direct
/// `[base + displacement]` operand forms via `search_instructions`) — these keys
/// must NOT divert a shot off authoritative flight, because native never reads
/// them in the flight loop at all:
/// - `Bouncy=` (`+0x2A7`) has **no consumer anywhere in the binary**. The
///   apparent hits at `0x0070D2D2`/`0x0070D2EE` are `TechnoTypeClass+0x2A7`
///   behind `VeterancyClass::IsVeteran/IsElite`. Stock `[Lobbed]` (the dog
///   discus) is therefore an ordinary `Arcing` shell.
/// - `Proximity=` (`+0x29F`) likewise has no consumer; `0x00702BC0`ff. are
///   `TechnoTypeClass+0x29F` behind the same veterancy pattern.
/// - `SubjectToCliffs=` (`+0x296`) is consumed only by the AI collision probe
///   `FUN_00468BB0 @ 0x00468BEC`, and `SubjectToElevation=` (`+0x297`) only by
///   `TechnoClass::InRange @ 0x006F72EF`/`0x006F7459` — a targeting key, not a
///   flight key.
/// - `VeryHigh=` (`+0x299`) is read only as a `HomingTrack` argument on the
///   `ROT >= 1` arm, so `VeryHigh` with `ROT <= 0` is unreachable; the one
///   stock user `[ChemMissile]` has `ROT=4`.
/// - `Degenerates=` (`+0x2A6`) IS live code — `BulletClass::AI 0x00467C86`
///   decrements the bullet's damage on every non-detonating frame while it
///   exceeds 5 — but stock `rulesmd.ini` has zero users, so it is recorded and
///   not implemented.
/// - `Elasticity=` (`+0x2C8`) is live in the ARM B reflection block but has
///   zero stock projectile users (the `[PIECE]`/`[TIRE]` hits are VoxelAnims).
///
/// `BulletClass::Fire` takes the second scatter site, for an `Inviso` and
/// `FlakScatter` bullet ([`inviso_fire_placement`]).
///
/// Ordinary `ROT < 1, Vertical = no` AI subtracts gravity every visit
/// (467402..467429), independently of `Arcing`. Production gives all such
/// persistent shots the ordinary gravity/collision arm. Scalar FireAt math
/// retains binary64 velocity; upstream FLH/pivot and homing producers remain
/// explicitly bounded in their owners.
pub(super) fn classify_projectile_delivery(
    weapon: &crate::rules::weapon_type::WeaponType,
    rules: &RuleSet,
) -> ProjectileDelivery {
    classify_projectile_type(bullet_type(weapon, rules), weapon.speed, rules)
}

fn classify_projectile_type(
    projectile: &crate::rules::projectile_type::ProjectileType,
    max_speed: i32,
    rules: &RuleSet,
) -> ProjectileDelivery {
    // `BulletClass::AI @ 0x004666E0` selects an arm exactly twice: `ROT < 1` at
    // `0x004668D1`, then `Vertical` (`+0x2C0`) at `0x004671D0`. Nothing else
    // participates.
    let ballistic = projectile.arcing;
    ProjectileDelivery {
        inviso: projectile.inviso,
        arm_frames: projectile.arm,
        tracks_target: projectile.rot > 0,
        collision: ProjectileCollisionPolicy {
            level: projectile.level,
            subject_to_walls: projectile.subject_to_walls,
            native_cell_collision: projectile.rot <= 0 && !projectile.vertical,
            dropping: projectile.dropping,
            subject_to_cliffs: projectile.subject_to_cliffs,
            flak_scatter: projectile.flak_scatter,
            anti_air: projectile.aa,
            airburst: projectile.airburst,
            inaccurate: projectile.inaccurate,
            floater: projectile.floater,
            elasticity_bits: projectile.elasticity.to_bits(),
            arcing: projectile.arcing,
        },
        ballistic,
        vertical: projectile
            .vertical
            .then_some(projectile.detonation_altitude),
        acceleration: projectile.acceleration,
        // `0x006FE67D`/`0x006FE68B`: the outer gate is `Inaccurate && Arcing`.
        // Inside, `FlakScatter && !Inviso` takes the range-scaled arm at
        // `0x006FE6AD` and everything else the plain arm at `0x006FE7FE`.
        launch_scatter_is_flak: (projectile.inaccurate && projectile.arcing)
            .then_some(projectile.flak_scatter && !projectile.inviso),
        guidance: (projectile.rot > 0).then_some(ProjectileGuidance {
            rot: projectile.rot,
            missile_rot_var: crate::util::native_x87::NativeF64Bits::from_bits(
                rules.general.missile_rot_var.to_bits(),
            ),
            course_lock_duration: projectile.course_lock_duration,
            course_frames: 0,
            course_locked: true,
            very_high: projectile.very_high,
            max_speed,
            acceleration: projectile.acceleration,
            closing_frames: 0,
            closing_accumulator_bits: 0,
        }),
    }
}

/// One bullet as `BulletClass::Construct @ 0x004664C0` builds it and its
/// launch, `BulletClass::Fire @ 0x00468670` (vt+0x1F0), places it, whoever
/// fires it: FireAt, a silo's nuclear missile (`0x0044CAED`), the warhead
/// `NukeMaker=` drops (`0x0046B408`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct FiredBullet {
    /// The AbstractClass identity the constructor took.
    pub(super) native_unique_id: i32,
    /// Construct's owner (`+0xB0`).
    pub(super) source_id: u64,
    /// Construct's target (`+0x10C`).
    pub(super) target: ProjectileTarget,
    /// The target coordinate Fire keeps (`+0x140`), also a guided bullet's
    /// fuse reference.
    pub(super) target_position: ProjectileCoord,
    pub(super) payload: ProjectilePayload,
    /// Construct's speed (`+0x110`): the ceiling of a Vertical or guided
    /// flight.
    pub(super) max_speed: i32,
    /// Fire's coordinate and velocity.
    pub(super) origin: ProjectileCoord,
    pub(super) velocity: crate::sim::projectile::ProjectileVelocity,
    /// The current speed.
    pub(super) speed: u16,
}

impl FiredBullet {
    /// A bullet `BulletClass::Fire` launches from `origin` at `velocity` and
    /// that moves at the velocity's magnitude, as the silo's missile and
    /// NukeMaker's warhead do (FireAt keeps its own launch speed and its own
    /// read of the aim). Fire keeps the target's vt+0x58 (`0x00468707`, into
    /// `+0x140`): a cell's aim, raised over a structural bridge deck
    /// ([`cell_target_coord`]), the shared dummy cell's, or an object's
    /// GetCoords (Object vt+0x58 forwards vt+0x48). `None` for a missing
    /// target, which native dereferences.
    ///
    /// [`cell_target_coord`]: crate::sim::projectile::cell_target_coord
    pub(crate) fn launched(
        world: &Simulation,
        native_unique_id: i32,
        source_id: u64,
        target: ProjectileTarget,
        payload: ProjectilePayload,
        max_speed: i32,
        origin: ProjectileCoord,
        velocity: crate::sim::projectile::ProjectileVelocity,
    ) -> Option<Self> {
        let terrain = world.resolved_terrain.as_ref();
        let target_position = match target {
            ProjectileTarget::Cell { rx, ry } => {
                crate::sim::projectile::cell_target_coord(terrain, rx, ry)
            }
            ProjectileTarget::DummyCell => crate::sim::projectile::dummy_cell_target_coord(
                &world.effective_shared_cell_dummy(),
            ),
            ProjectileTarget::Entity(id) => {
                let coords = crate::sim::movement::ground_pose::object_get_coords(
                    world.substrate.entities.get(id)?,
                    terrain,
                );
                ProjectileCoord::new(coords.x, coords.y, coords.z)
            }
            // Terrain+58 (410540) forwards Object+48 (5F65A0).
            ProjectileTarget::Terrain(id) => {
                world.production.terrain_objects.get(&id)?.world_coord()
            }
            ProjectileTarget::None => return None,
        };
        Some(Self {
            native_unique_id,
            source_id,
            target,
            target_position,
            payload,
            max_speed,
            origin,
            velocity,
            speed: crate::sim::projectile::projectile_velocity_magnitude(velocity)
                .clamp(0.0, f64::from(u16::MAX)) as u16,
        })
    }
}

impl ProjectileDelivery {
    /// The spawn of `bullet`, a bullet of this delivery's BulletType
    /// (`projectile_type`, absent for a weapon naming none).
    fn spawn(
        &self,
        entities: &EntityStore,
        rules: &RuleSet,
        projectile_type: Option<&crate::rules::projectile_type::ProjectileType>,
        bullet: FiredBullet,
    ) -> ProjectileSpawn {
        let guidance = self.guidance.map(|guidance| ProjectileGuidance {
            max_speed: bullet.max_speed,
            ..guidance
        });
        ProjectileSpawn {
            native_unique_id: bullet.native_unique_id,
            line_trail: projectile_type.and_then(|kind| {
                crate::sim::projectile::ProjectileLineTrail::from_type(
                    kind,
                    rules.general.line_trail_color_override,
                )
            }),
            flat: projectile_type.is_some_and(|projectile| projectile.flat),
            source_id: bullet.source_id,
            origin: bullet.origin,
            target: bullet.target,
            initial_target_position: bullet.target_position,
            payload: bullet.payload,
            speed_leptons_per_frame: bullet.speed,
            velocity: bullet.velocity,
            trajectory: match self.vertical {
                Some(detonation_altitude) => ProjectileTrajectory::Vertical {
                    detonation_altitude,
                    acceleration: self.acceleration,
                    max_speed: bullet.max_speed,
                },
                None if self.ballistic || guidance.is_none() => ProjectileTrajectory::Ballistic,
                None => ProjectileTrajectory::Straight,
            },
            guidance,
            visual: projectile_type.map_or(ProjectileVisualState::new(0, 0, 0), |projectile| {
                ProjectileVisualState::new(
                    projectile.anim_low as u8,
                    projectile.anim_high as u8,
                    projectile.anim_rate as u8,
                )
            }),
            arm_frames: projectile_arm_delay(self.arm_frames, bullet.target, entities),
            fuse_frames: None,
            // AI 467C0C calls Check for ROT>0 or Ranged even when
            // Dropping later suppresses detector-only admission.
            ranged_fuse: self.tracks_target
                || projectile_type.is_some_and(|projectile| projectile.ranged),
            tracks_target: self.tracks_target,
            target_expiry: TargetExpiryPolicy::DetonateAtLastKnown,
            collision: self.collision,
        }
    }
}

/// Admit `bullet`, fired with `weapon`'s BulletType, as `bullet_id`. Fire
/// Unlimbos it at its coordinate, which appends it to the Logic vector
/// (`0x005F5040`): it takes its first AI later in this frame's pass. An
/// `Inviso=` bullet then stands on its placement ([`inviso_fire_placement`],
/// [`ProjectileStore::fire_inviso`]) with the target's OnBridge, as FireAt
/// copies it at `0x006FF08B`.
///
/// [`ProjectileStore::fire_inviso`]: crate::sim::projectile::ProjectileStore::fire_inviso
pub(crate) fn admit_fired_bullet(
    world: &mut Simulation,
    rules: &RuleSet,
    weapon: &WeaponType,
    bullet_id: u64,
    bullet: FiredBullet,
) {
    let projectile_type = bullet
        .payload
        .projectile_type()
        .and_then(|id| rules.projectile(world.interner.resolve(id)));
    let delivery = classify_projectile_type(
        projectile_type.unwrap_or_else(|| missing_projectile_fallback()),
        bullet.max_speed,
        rules,
    );
    let spawn = delivery.spawn(&world.substrate.entities, rules, projectile_type, bullet);
    world.admit_projectile(bullet_id, spawn);
    world.construct_bullet_scheme(bullet_id, rules);
    #[cfg(test)]
    if let Some(fixture) = world.receiver_fixture.as_mut() {
        fixture
            .admitted_spawns
            .push((bullet_id, delivery.inviso, spawn));
    }
    if delivery.inviso {
        let on_bridge = match bullet.target {
            ProjectileTarget::Entity(id) => world
                .substrate
                .entities
                .get(id)
                .is_some_and(|target| target.on_bridge()),
            _ => false,
        };
        let placement = inviso_fire_placement(
            world,
            rules,
            delivery.collision.flak_scatter,
            weapon,
            bullet.origin,
            bullet.target_position,
        );
        world
            .projectiles
            .fire_inviso(bullet_id, placement, on_bridge);
    }
}

/// `BulletClass::Fire`'s placement for its Inviso arm: the target coordinate
/// it kept (`+0x140`), which a `FlakScatter=` bullet first moves
/// (`0x00468734..0x004688A5`,
/// [`crate::sim::projectile::launch::bullet_fire_flak_placement`]: two
/// Scenario draws). `weapon` is the bullet's WeaponType (`+0x130`, which
/// FireAt sets to its fired weapon at `0x006FE573`) and `origin` the
/// coordinate Fire was given.
/// SpawnShrapnel's Fire leaves its children on their target
/// (`Simulation::admit_projectile_spawns`).
fn inviso_fire_placement(
    world: &mut Simulation,
    rules: &RuleSet,
    flak_scatter: bool,
    weapon: &WeaponType,
    origin: ProjectileCoord,
    target: ProjectileCoord,
) -> ProjectileCoord {
    if !flak_scatter {
        return target;
    }
    crate::sim::projectile::launch::bullet_fire_flak_placement(
        origin,
        target,
        rules.combat_damage.ballistic_scatter,
        weapon.range_leptons,
        &mut world.scenario_rng,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    /// `BulletClass::AI @ 0x004666E0` branches only on `ROT < 1` and, on the
    /// non-homing arm, on `Vertical`. `Proximity`, `SubjectToCliffs` and
    /// `SubjectToElevation` are never read there, so none of them may push a
    /// shot off authoritative flight.
    #[test]
    fn gsi_08_07_proximity_and_cliff_keys_keep_authoritative_flight() {
        let ini = IniFile::from_str(
            "[VehicleTypes]\n0=PROXER\n1=CLIFFER\n[PROXER]\nStrength=100\nArmor=heavy\nPrimary=ProxGun\n[CLIFFER]\nStrength=100\nArmor=heavy\nPrimary=CliffGun\n[ProxGun]\nDamage=10\nROF=20\nRange=5\nSpeed=40\nProjectile=Prox\nWarhead=WH\n[CliffGun]\nDamage=10\nROF=20\nRange=5\nSpeed=40\nProjectile=Cliffy\nWarhead=WH\n[Prox]\nProximity=yes\nROT=8\n[Cliffy]\nSubjectToCliffs=yes\nSubjectToElevation=yes\nROT=0\n[WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("projectile fixture parses");

        // Only ROT picks the homing arm; Proximity= and SubjectToCliffs= pick
        // no flight arm at all.
        for (weapon_name, homing) in [("ProxGun", true), ("CliffGun", false)] {
            let weapon = rules.weapon(weapon_name).expect("weapon");
            let delivery = classify_projectile_delivery(weapon, &rules);
            assert_eq!(delivery.tracks_target, homing, "{weapon_name}");
            assert_eq!(delivery.vertical, None, "{weapon_name}");
            assert!(!delivery.inviso, "{weapon_name}");
        }
    }

    #[test]
    fn wall_projectile_uses_the_authoritative_collision_path() {
        let ini = IniFile::from_str(
            "[VehicleTypes]\n0=TEST\n\n[TEST]\nStrength=100\nArmor=heavy\nPrimary=GUN\n\n[GUN]\nDamage=20\nROF=10\nRange=5\nSpeed=30\nProjectile=SHELL\nWarhead=WH\n\n[SHELL]\nSubjectToWalls=yes\n\n[WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("projectile rules");
        let weapon = rules.weapon("GUN").expect("weapon");

        assert_eq!(
            classify_projectile_delivery(weapon, &rules),
            ProjectileDelivery {
                inviso: false,
                arm_frames: 0,
                tracks_target: false,
                collision: ProjectileCollisionPolicy {
                    level: false,
                    subject_to_walls: true,
                    native_cell_collision: true,
                    ..ProjectileCollisionPolicy::NONE
                },
                ballistic: false,
                vertical: None,
                acceleration: 3,
                launch_scatter_is_flak: None,
                guidance: None,
            }
        );
    }

    /// `Bouncy=` (`+0x2A7`), `Proximity=` (`+0x29F`) and `Degenerates=`
    /// (`+0x2A6`) never divert a shot: the first two have no consumer anywhere
    /// in `gamemd.exe`, and the third is a damage decay inside
    /// `BulletClass::AI 0x00467C86`, not a trajectory selector. `Inaccurate=`
    /// and `FlakScatter=` are launch-time target offsets at
    /// `TechnoClass::FireAt 0x006FE67D`, and `Dropping=` (`+0x29C`) only
    /// suppresses the proximity fuse at `0x00467C78`.
    #[test]
    fn gsi_08_08_dead_and_launch_time_keys_keep_authoritative_flight() {
        let ini = IniFile::from_str(
            "[VehicleTypes]\n0=TA\n1=TB\n\
             [TA]\nStrength=100\nArmor=heavy\nPrimary=W0\nSecondary=W1\n\
             [TB]\nStrength=100\nArmor=heavy\nPrimary=W2\nSecondary=W3\n\
             [W0]\nDamage=10\nROF=20\nRange=5\nSpeed=40\nProjectile=P0\nWarhead=WH\n\
             [W1]\nDamage=10\nROF=20\nRange=5\nSpeed=40\nProjectile=P1\nWarhead=WH\n\
             [W2]\nDamage=10\nROF=20\nRange=5\nSpeed=40\nProjectile=P2\nWarhead=WH\n\
             [W3]\nDamage=10\nROF=20\nRange=5\nSpeed=40\nProjectile=P3\nWarhead=WH\n\
             [P0]\nBouncy=yes\nArcing=yes\n[P1]\nDegenerates=yes\n\
             [P2]\nInaccurate=yes\nFlakScatter=yes\nArcing=true\n[P3]\nDropping=yes\nROT=4\n\
             [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("projectile fixture parses");
        // Bouncy=, Degenerates=, Inaccurate=/FlakScatter= and Dropping= select
        // no flight arm: ROT alone does.
        for (weapon_name, homing) in [("W0", false), ("W1", false), ("W2", false), ("W3", true)] {
            let weapon = rules.weapon(weapon_name).expect("weapon");
            let delivery = classify_projectile_delivery(weapon, &rules);
            assert_eq!(delivery.tracks_target, homing, "{weapon_name}");
            assert_eq!(delivery.vertical, None, "{weapon_name}");
        }
        // The flak arm is only selected when `FlakScatter && !Inviso`.
        let flak = rules.weapon("W2").expect("weapon");
        assert!(matches!(
            classify_projectile_delivery(flak, &rules),
            ProjectileDelivery {
                launch_scatter_is_flak: Some(true),
                ..
            }
        ));
        // `Bouncy` + `Arcing` is an ordinary ballistic shell, with no scatter.
        let bouncy = rules.weapon("W0").expect("weapon");
        assert!(matches!(
            classify_projectile_delivery(bouncy, &rules),
            ProjectileDelivery {
                ballistic: true,
                launch_scatter_is_flak: None,
                ..
            }
        ));
    }

    /// `Vertical=` selects the third `BulletClass::AI` arm and carries
    /// `DetonationAltitude=` (`+0x2BC`) and `Acceleration=` (`+0x2D0`).
    #[test]
    fn gsi_08_08_vertical_projectile_takes_the_vertical_arm() {
        let ini = IniFile::from_str(
            "[VehicleTypes]\n0=T\n[T]\nStrength=100\nArmor=heavy\nPrimary=NUKE\n\
             [NUKE]\nDamage=10\nROF=20\nRange=5\nSpeed=40\nProjectile=GiantNukeUp\nWarhead=WH\n\
             [GiantNukeUp]\nArm=2\nAcceleration=1\nVertical=yes\nDetonationAltitude=20000\n\
             [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("projectile fixture parses");
        let weapon = rules.weapon("NUKE").expect("weapon");
        assert!(matches!(
            classify_projectile_delivery(weapon, &rules),
            ProjectileDelivery {
                vertical: Some(20000),
                acceleration: 1,
                arm_frames: 2,
                guidance: None,
                ballistic: false,
                ..
            }
        ));
    }
}
