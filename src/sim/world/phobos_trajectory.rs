//! Phobos `PhobosTrajectory` dispatch and `StraightTrajectory` flight
//! (`Ext/Bullet/Trajectories/PhobosTrajectory.cpp:436-608`,
//! `StraightTrajectory.cpp:140-1061`).
//!
//! The native hooks replace the stock `BulletClass::AI` checks for a bullet
//! whose BulletType names a trajectory: `OnAI` runs first and may detonate,
//! `OnAIVelocity` cancels gravity, the target-coordinate and techno checks
//! are skipped, and `OnAIPreDetonate` runs at the detonation. VERA runs the
//! Straight arm here instead of [`crate::sim::projectile`]'s stock arms and
//! commits its detonations through the same `ProjectileDetonation` boundary.
//!
//! ## RNG draws
//! - `PrepareForOpenFire`'s `Inaccurate=` scatter: `RandomRanged` magnitude
//!   then the direction draw (see [`crate::sim::projectile::launch`]).
//! - `BulletDetonateVelocityCheck` and the proximity scan draw nothing.
//! - `PassDetonate`/`Proximity` warhead detonations go through the combat
//!   owner, which draws as any warhead impact does.
//!
//! ## Dependency rules
//! - Part of sim/; no render/ui/audio/net. Uses the world's collision owner.

#[cfg(feature = "phobos")]
#[path = "phobos_trajectory_ai.rs"]
mod ai;
#[cfg(feature = "phobos")]
#[path = "phobos_trajectory_bombard.rs"]
mod bombard;
#[cfg(feature = "phobos")]
#[path = "phobos_trajectory_parabola.rs"]
mod parabola;

use std::collections::BTreeMap;

use super::Simulation;
use crate::rules::phobos_trajectory::{StraightTrajectoryRules, TrajectoryFlag};
use crate::rules::ruleset::RuleSet;
use crate::rules::weapon_type::WeaponType;
use crate::sim::combat::{EntityDamageEvent, ReceiverCallFlags};
use crate::sim::intern::InternedId;
use crate::sim::projectile::{
    Projectile, ProjectileCoord, ProjectileDetonation, ProjectileDetonationReason,
    ProjectilePayload, ProjectileTarget, ProjectileVelocity,
};
use crate::sim::timer::CdTimer;
use crate::util::native_x87::{NativeF32Bits, NativeF64Bits, X87Chop53};

/// One bullet's `StraightTrajectory` instance (`StraightTrajectory.h:83-135`).
/// `Type` is read from the BulletType at use; every mutable field mirrors the
/// native instance so save, hash and replay stay exact.
#[cfg(feature = "phobos")]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct PhobosTrajectoryState {
    pub kind: TrajectoryFlag,
    /// `DetonationDistance`, range-modified at Unlimbo when asked.
    pub detonation_distance: i32,
    /// `PassDetonateDamage`, attenuated per hit.
    pub pass_detonate_damage: i32,
    /// `PassDetonateTimer` (`CDTimerClass`).
    pub pass_detonate_timer: CdTimer,
    /// `OffsetCoord`, mirrored on an odd burst.
    pub offset_coord: [i32; 3],
    pub use_disperse_burst: bool,
    /// `ProximityImpact`, decremented per proximity hit.
    pub proximity_impact: i32,
    /// `ProximityDamage`, attenuated per hit.
    pub proximity_damage: i32,
    /// `RemainingDistance`, starts at 1.
    pub remaining_distance: i32,
    /// `ExtraCheck`: the obstacle/techno a later frame detonates on.
    pub extra_check: Option<u64>,
    /// `TheCasualty`: native `map<int,int>` of `UniqueID` to remaining
    /// frames; VERA keys it by the stable object id.
    pub the_casualty: BTreeMap<u64, i32>,
    /// `FirepowerMult`, the firer's current firepower multiplier at Unlimbo.
    pub firepower_mult: NativeF64Bits,
    /// `AttenuationRange`: the weapon `Range=` (modified when asked).
    pub attenuation_range: i32,
    /// `LastTargetCoord`.
    pub last_target_coord: ProjectileCoord,
    /// `CurrentBurst` / `CountOfBurst` at Unlimbo.
    pub current_burst: i32,
    pub count_of_burst: i32,
    /// `WaitOneFrame`: 2 waits for the target's first update, 1 consumed it.
    pub wait_one_frame: i32,
    /// Phobos `BulletExt::SnappedToTarget`: set by `OnAIPreDetonate` when the
    /// impact coordinate snapped onto the target.
    #[serde(default)]
    pub snapped_to_target: bool,
    /// `BombardTrajectory::Height`, `height + TargetCoords.Z` at Unlimbo.
    #[serde(default = "zero_bits")]
    pub bombard_height: NativeF64Bits,
    /// `BombardTrajectory::FallPercent`, shifted by the type value and the
    /// one `RandomRanged(0, 200 * FallPercentShift)` draw at Unlimbo.
    #[serde(default = "zero_bits")]
    pub bombard_fall_percent: NativeF64Bits,
    /// `BombardTrajectory::InitialTargetCoord`.
    #[serde(default = "zero_coord")]
    pub initial_target_coord: ProjectileCoord,
    /// `BombardTrajectory::IsFalling` / `ToFalling`.
    #[serde(default)]
    pub is_falling: bool,
    #[serde(default)]
    pub to_falling: bool,
    /// `BombardTrajectory::RotateAngle` (binary64 radians).
    #[serde(default = "zero_bits")]
    pub rotate_angle: NativeF64Bits,
    /// `ParabolaTrajectory::ThrowHeight`, 600 when the type value is `<= 0`.
    #[serde(default)]
    pub throw_height: i32,
    /// `ParabolaTrajectory::BounceTimes` left.
    #[serde(default)]
    pub bounce_times: i32,
    /// `ParabolaTrajectory::ShouldDetonate` / `ShouldBounce` / `NeedExtraCheck`.
    #[serde(default)]
    pub should_detonate: bool,
    #[serde(default)]
    pub should_bounce: bool,
    #[serde(default)]
    pub need_extra_check: bool,
    /// `ParabolaTrajectory::LastVelocity` used by the bounce reflection.
    #[serde(default = "zero_velocity")]
    pub last_velocity: [NativeF64Bits; 3],
}

#[cfg(feature = "phobos")]
fn zero_bits() -> NativeF64Bits {
    NativeF64Bits::from_bits(0)
}

#[cfg(feature = "phobos")]
fn zero_coord() -> ProjectileCoord {
    ProjectileCoord::new(0, 0, 0)
}

#[cfg(feature = "phobos")]
fn zero_velocity() -> [NativeF64Bits; 3] {
    [NativeF64Bits::from_bits(0); 3]
}

/// Deterministic fold of the per-bullet trajectory instance; only a bullet
/// that carries one contributes.
#[cfg(feature = "phobos")]
impl PhobosTrajectoryState {
    /// The instance fields a class constructor leaves at zero before
    /// `OnUnlimbo` fills them ([`crate::sim::world::phobos_trajectory`]).
    fn base(kind: TrajectoryFlag) -> Self {
        Self {
            kind,
            detonation_distance: 0,
            pass_detonate_damage: 0,
            pass_detonate_timer: CdTimer::default(),
            offset_coord: [0; 3],
            use_disperse_burst: false,
            proximity_impact: 0,
            proximity_damage: 0,
            remaining_distance: 1,
            extra_check: None,
            the_casualty: BTreeMap::new(),
            firepower_mult: NativeF64Bits::ONE,
            attenuation_range: 0,
            last_target_coord: zero_coord(),
            current_burst: 0,
            count_of_burst: 0,
            wait_one_frame: 0,
            snapped_to_target: false,
            bombard_height: zero_bits(),
            bombard_fall_percent: zero_bits(),
            initial_target_coord: zero_coord(),
            is_falling: false,
            to_falling: false,
            rotate_angle: zero_bits(),
            throw_height: 0,
            bounce_times: 0,
            should_detonate: false,
            should_bounce: false,
            need_extra_check: false,
            last_velocity: zero_velocity(),
        }
    }

    pub(crate) fn hash_state(&self, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        self.kind.hash(hasher);
        self.detonation_distance.hash(hasher);
        self.pass_detonate_damage.hash(hasher);
        self.pass_detonate_timer.hash(hasher);
        self.offset_coord.hash(hasher);
        self.use_disperse_burst.hash(hasher);
        self.proximity_impact.hash(hasher);
        self.proximity_damage.hash(hasher);
        self.remaining_distance.hash(hasher);
        self.extra_check.hash(hasher);
        self.the_casualty.hash(hasher);
        self.firepower_mult.hash(hasher);
        self.attenuation_range.hash(hasher);
        self.last_target_coord.hash(hasher);
        self.current_burst.hash(hasher);
        self.count_of_burst.hash(hasher);
        self.wait_one_frame.hash(hasher);
        self.snapped_to_target.hash(hasher);
        self.bombard_height.hash(hasher);
        self.bombard_fall_percent.hash(hasher);
        self.initial_target_coord.hash(hasher);
        self.is_falling.hash(hasher);
        self.to_falling.hash(hasher);
        self.rotate_angle.hash(hasher);
        self.throw_height.hash(hasher);
        self.bounce_times.hash(hasher);
        self.should_detonate.hash(hasher);
        self.should_bounce.hash(hasher);
        self.need_extra_check.hash(hasher);
        self.last_velocity.hash(hasher);
    }
}

/// What one trajectory visit produced for the world to commit.
#[cfg(feature = "phobos")]
#[derive(Default)]
pub(super) struct TrajectoryStep {
    /// Detonations that end the bullet's flight (the stock detonation arm).
    pub detonations: Vec<ProjectileDetonation>,
    /// Warhead detonations that leave the bullet flying (`PassDetonate`,
    /// `Proximity`, `BounceDetonate`; `StraightTrajectory.cpp:739-976`,
    /// `ParabolaTrajectory.cpp:882-887`).
    pub extra_detonations: Vec<ProjectileDetonation>,
    pub direct_damage: Vec<EntityDamageEvent>,
    /// The bullet left the map (`left_map`): it is UnInit'ed, not detonated.
    pub expired: bool,
}

#[cfg(feature = "phobos")]
impl Simulation {
    /// `TrajectoryTypePointer::CreateInstance` + `OnUnlimbo`
    /// (`PhobosTrajectory.cpp:7-22,436-608`), dispatched by the `Trajectory`
    /// flag; each class then runs its immediate `PrepareForOpenFire` or the
    /// one-frame lead wait.
    pub(crate) fn init_phobos_trajectory(
        &mut self,
        bullet_id: u64,
        rules: &RuleSet,
        weapon: &WeaponType,
    ) {
        let Some(projectile) = self.projectiles.get(bullet_id) else {
            return;
        };
        let Some(type_id) = projectile.payload().projectile_type() else {
            return;
        };
        let type_name = self.interner.resolve(type_id).to_owned();
        let Some(kind) = rules.projectile(&type_name) else {
            return;
        };
        match kind.phobos_trajectory.flag {
            Some(TrajectoryFlag::Straight) => {
                self.init_straight_trajectory(bullet_id, rules, weapon)
            }
            Some(TrajectoryFlag::Bombard) => self.init_bombard_trajectory(bullet_id, rules, weapon),
            Some(TrajectoryFlag::Parabola) => {
                self.init_parabola_trajectory(bullet_id, rules, weapon)
            }
            None => {}
        }
    }

    /// `StraightTrajectory::OnUnlimbo` (`StraightTrajectory.cpp:140-180`) and
    /// the immediate `PrepareForOpenFire` (or the one-frame lead wait), run
    /// through [`Simulation::admit_fired_bullet`]'s spawn.
    fn init_straight_trajectory(&mut self, bullet_id: u64, rules: &RuleSet, weapon: &WeaponType) {
        let Some(projectile) = self.projectiles.get(bullet_id) else {
            return;
        };
        let Some(type_id) = projectile.payload().projectile_type() else {
            return;
        };
        let type_name = self.interner.resolve(type_id).to_owned();
        let Some(kind) = rules.projectile(&type_name) else {
            return;
        };
        let base = &kind.phobos_trajectory;
        let t = &base.straight;
        let source_id = projectile.source_id();
        let launch_target = projectile.launch_target();
        let target = projectile.target();
        let frame = self.session.binary_frame() as i32;

        let mut state = PhobosTrajectoryState {
            kind: TrajectoryFlag::Straight,
            detonation_distance: t.detonation_distance,
            pass_detonate_damage: t.pass_detonate_damage,
            pass_detonate_timer: CdTimer::started(frame, t.pass_detonate_initial_delay.max(0)),
            offset_coord: t.offset_coord,
            use_disperse_burst: t.use_disperse_burst,
            proximity_impact: t.proximity_impact,
            proximity_damage: t.proximity_damage,
            remaining_distance: 1,
            extra_check: None,
            the_casualty: BTreeMap::new(),
            firepower_mult: NativeF64Bits::ONE,
            attenuation_range: 0,
            last_target_coord: launch_target,
            current_burst: 0,
            count_of_burst: 0,
            wait_one_frame: 0,
            snapped_to_target: false,
            ..PhobosTrajectoryState::base(TrajectoryFlag::Straight)
        };
        state.attenuation_range = weapon.range_leptons;
        state.count_of_burst = weapon.burst;
        if let Some(firer) = self.substrate.entities.get(source_id) {
            if t.apply_range_modifiers {
                let range_with = |range: i32| {
                    crate::sim::attach_effect::get_range_with_modifiers(
                        rules,
                        &self.interner,
                        Some(&self.substrate.entities),
                        firer,
                        &weapon.id,
                        range,
                    )
                };
                state.detonation_distance = if state.detonation_distance >= 0 {
                    range_with(state.detonation_distance)
                } else {
                    -range_with(-state.detonation_distance)
                };
                // The attenuation arm calls the two-argument overload
                // (`StraightTrajectory.cpp:161`), which owns the -512 sentinel.
                state.attenuation_range = crate::sim::attach_effect::weapon_range_with_modifiers(
                    rules,
                    &self.interner,
                    Some(&self.substrate.entities),
                    firer,
                    weapon,
                );
            }
            state.current_burst = firer.weapon_burst.index();
            state.firepower_mult = NativeF64Bits::from_bits(
                crate::sim::combat::crit::current_firepower_multiplier(self, rules, firer)
                    .to_bits(),
            );
            if t.mirror_coord && state.current_burst % 2 == 1 {
                state.offset_coord[1] = state.offset_coord[1].wrapping_neg();
            }
        }

        let target_is_foot = match target {
            ProjectileTarget::Entity(id) => self.substrate.entities.get(id).is_some_and(|entity| {
                matches!(
                    entity.category(),
                    crate::map::entities::EntityCategory::Infantry
                        | crate::map::entities::EntityCategory::Unit
                        | crate::map::entities::EntityCategory::Aircraft
                )
            }),
            _ => false,
        };
        let wait = t.lead_time_calculate && target_is_foot;
        if wait {
            state.wait_one_frame = 2;
        }
        if let Some(projectile) = self.projectiles.get_mut(bullet_id) {
            projectile.set_velocity(ProjectileVelocity::new(0, 0, 0));
            projectile.set_phobos_trajectory(Some(state));
        }
        if !wait {
            self.prepare_phobos_open_fire(bullet_id, rules, t, base.speed, kind.inaccurate);
        }
    }

    /// `StraightTrajectory::PrepareForOpenFire` (`StraightTrajectory.cpp:258-423`):
    /// lead-time solve, fixed offset, `Inaccurate=` scatter, remaining
    /// distance, velocity and burst rotation.
    fn prepare_phobos_open_fire(
        &mut self,
        bullet_id: u64,
        rules: &RuleSet,
        t: &StraightTrajectoryRules,
        speed: f64,
        inaccurate: bool,
    ) {
        let terrain = self.resolved_terrain.as_ref();
        let Some(projectile) = self.projectiles.get(bullet_id) else {
            return;
        };
        let target_ref = projectile.target();
        let source_id = projectile.source_id();
        let bullet_target = projectile.last_target_position();
        let bullet_source = projectile.launch_origin();
        let bullet_location = projectile.position();
        let mut the_target = bullet_target;
        let mut the_source = bullet_source;

        if t.lead_time_calculate {
            if let ProjectileTarget::Entity(id) = target_ref
                && let Some(target) = self.substrate.entities.get(id)
            {
                let coords = crate::sim::movement::ground_pose::object_get_coords(target, terrain);
                the_target = ProjectileCoord::new(coords.x, coords.y, coords.z);
                the_source = bullet_location;
                if the_target != projectile.last_target_position() {
                    let extra = sub(the_target, projectile.last_target_position());
                    let target_source = sub(the_source, the_target);
                    let last_source = sub(the_source, projectile.last_target_position());
                    let distance_sq = mag_sq(target_source);
                    let target_speed_sq = mag_sq(extra);
                    let target_speed = (target_speed_sq as f64).sqrt();
                    let cross = cross(target_source, last_source);
                    let vertical_sq = mag_sq(cross) as f64 / target_speed_sq as f64;
                    let horizon_sq = distance_sq as f64 - vertical_sq;
                    let horizon = horizon_sq.max(0.0).sqrt();
                    let straight_sq = speed * speed;
                    let base = straight_sq - target_speed_sq as f64;
                    let square = base * vertical_sq + straight_sq * horizon_sq;
                    if square > 1e-10 {
                        let minus = -horizon * target_speed;
                        let mut travel_time = 0i32;
                        if base.abs() < 1e-10 {
                            travel_time = if horizon.abs() > 1e-10 {
                                ((distance_sq as f64 / (2.0 * horizon * target_speed)) as i32) + 1
                            } else {
                                0
                            };
                        } else {
                            let travel_m = ((minus - square.sqrt()) / base) as i32;
                            let travel_p = ((minus + square.sqrt()) / base) as i32;
                            if travel_m > 0 && travel_p > 0 {
                                travel_time = travel_m.min(travel_p);
                            } else if travel_m > 0 {
                                travel_time = travel_m;
                            } else if travel_p > 0 {
                                travel_time = travel_p;
                            }
                            travel_time += if mag_sq(target_source) < mag_sq(last_source) {
                                1
                            } else {
                                2
                            };
                        }
                        the_target = add(the_target, scale(extra, travel_time as i32));
                    }
                }
            }
        }

        let rotate_angle = if !t.lead_time_calculate
            && the_target == the_source
            && let Some(owner) = self.substrate.entities.get(source_id)
        {
            let owner_coords = crate::sim::movement::ground_pose::object_get_coords(owner, terrain);
            atan2(
                the_target.y.wrapping_sub(owner_coords.y),
                the_target.x.wrapping_sub(owner_coords.x),
            )
        } else {
            atan2(
                the_target.y.wrapping_sub(the_source.y),
                the_target.x.wrapping_sub(the_source.x),
            )
        };
        let (sin_a, cos_a) = sin_cos(rotate_angle);

        if t.offset_coord != [0; 3] {
            let [ox, oy, oz] = t.offset_coord;
            the_target.x = the_target
                .x
                .wrapping_add((f64::from(ox) * cos_a + f64::from(oy) * sin_a) as i32);
            the_target.y = the_target
                .y
                .wrapping_add((f64::from(ox) * sin_a - f64::from(oy) * cos_a) as i32);
            the_target.z = the_target.z.wrapping_add(oz);
        }

        if inaccurate {
            let distance = distance(the_source, the_target);
            let offset_mult = 0.0004 * distance;
            let (min, max) = ballistic_scatter_bounds(rules, &projectile, self);
            let offset_min = (offset_mult * f64::from(min)) as i32;
            let offset_max = (offset_mult * f64::from(max)) as i32;
            let offset_distance = self
                .scenario_rng
                .next_range_i32_inclusive(offset_min, offset_max);
            the_target = crate::sim::projectile::launch::random_coords_near(
                self.cell_grid(),
                the_target,
                offset_distance,
                &mut self.scenario_rng,
            );
        }

        let horizontal = distance(the_source, the_target);
        let Some(instance) = self.projectiles.get(bullet_id) else {
            return;
        };
        let Some(mut state) = instance.phobos_trajectory().cloned() else {
            return;
        };
        if t.pass_through {
            if state.detonation_distance > 0 {
                state.remaining_distance = state
                    .remaining_distance
                    .wrapping_add((state.detonation_distance as f64 + speed) as i32);
            } else if state.detonation_distance < 0 {
                state.remaining_distance = state.remaining_distance.wrapping_add(
                    (horizontal - f64::from(state.detonation_distance) + speed) as i32,
                );
            } else {
                state.remaining_distance = i32::MAX;
            }
        } else {
            state.remaining_distance = state
                .remaining_distance
                .wrapping_add((horizontal + speed) as i32);
        }
        state.last_target_coord = the_target;

        let mut velocity = [
            f64::from(the_target.x.wrapping_sub(the_source.x)),
            f64::from(the_target.y.wrapping_sub(the_source.y)),
        ];
        let mut vz = if t.confine_at_height > 0 && t.pass_detonate_local {
            0.0
        } else {
            self.get_velocity_z(
                bullet_source,
                the_target,
                target_ref,
                source_id,
                &state,
                t,
                terrain,
            )
        };
        if !t.use_disperse_burst && t.rotate_coord.abs() > 1e-10 && state.count_of_burst > 1 {
            let rotated = burst_velocity(
                [velocity[0], velocity[1], vz],
                rotate_angle,
                t.axis_of_rotation,
                t.rotate_coord,
                t.mirror_coord,
                state.current_burst,
                state.count_of_burst,
                180.0,
            );
            velocity = [rotated[0], rotated[1]];
            vz = rotated[2];
        }
        let length = (velocity[0] * velocity[0] + velocity[1] * velocity[1] + vz * vz).sqrt();
        let (vx, vy, vz) = if length > 1e-10 {
            let scale = speed / length;
            (velocity[0] * scale, velocity[1] * scale, vz * scale)
        } else {
            state.remaining_distance = 0;
            (0.0, 0.0, 0.0)
        };
        if let Some(projectile) = self.projectiles.get_mut(bullet_id) {
            projectile.set_velocity(ProjectileVelocity::from_native([
                NativeF64Bits::from_bits(vx.to_bits()),
                NativeF64Bits::from_bits(vy.to_bits()),
                NativeF64Bits::from_bits(vz.to_bits()),
            ]));
            projectile.set_last_target_position(the_target);
            projectile.set_phobos_trajectory(Some(state));
        }
    }

    /// `StraightTrajectory::GetVelocityZ` (`StraightTrajectory.cpp:425-474`).
    #[allow(clippy::too_many_arguments)]
    fn get_velocity_z(
        &self,
        source_coords: ProjectileCoord,
        target_coords: ProjectileCoord,
        target: ProjectileTarget,
        source_id: u64,
        state: &PhobosTrajectoryState,
        t: &StraightTrajectoryRules,
        terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    ) -> f64 {
        let mut source_z = source_coords.z;
        let mut target_z = target_coords.z;
        let mut velocity_z = f64::from(target_z.wrapping_sub(source_z));
        if !t.pass_through {
            return velocity_z;
        }
        let cell_level = |rx: u16, ry: u16| {
            let mut z = crate::sim::projectile::cell_ground_coord(terrain, rx, ry).z;
            let cell = crate::sim::cell_rect::get_cellclass_fallback(
                terrain,
                i32::from(rx),
                i32::from(ry),
            );
            if cell.bridge_flags_0x1180() & 0x100 != 0 {
                z = z.wrapping_add(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS);
            }
            z
        };
        if let Some(owner) = self.substrate.entities.get(source_id) {
            source_z = cell_level(owner.position.rx(), owner.position.ry());
        }
        if let ProjectileTarget::Entity(id) = target
            && let Some(object) = self.substrate.entities.get(id)
        {
            target_z = cell_level(object.position.rx(), object.position.ry());
        }
        let _ = state;
        if source_z == target_z || velocity_z.abs() <= 32.0 {
            if state.detonation_distance == 0 {
                return 0.0;
            }
            let horizontal = distance(
                ProjectileCoord::new(source_coords.x, source_coords.y, 0),
                ProjectileCoord::new(target_coords.x, target_coords.y, 0),
            );
            let the_distance = if state.detonation_distance < 0 {
                horizontal - f64::from(state.detonation_distance)
            } else {
                f64::from(state.detonation_distance)
            };
            if the_distance.abs() > 1e-10 {
                velocity_z *= horizontal / the_distance;
            } else {
                return 0.0;
            }
        }
        velocity_z
    }
}

/// The two `BulletTypeExt::BallisticScatter_Min/Max` bounds. Phobos reads
/// Ares' BulletTypeExt; a Phobos-only build keeps the stock defaults
/// (`0` and `[CombatDamage] BallisticScatter=`).
fn ballistic_scatter_bounds(
    rules: &RuleSet,
    projectile: &Projectile,
    sim: &Simulation,
) -> (i32, i32) {
    let _ = (projectile, sim);
    #[cfg(feature = "ares")]
    if let Some(kind) = projectile
        .payload()
        .projectile_type()
        .and_then(|id| rules.projectile(sim.interner.resolve(id)))
    {
        if kind.bullet_type_ext.ballistic_scatter_min.is_some()
            || kind.bullet_type_ext.ballistic_scatter_max.is_some()
        {
            return (
                kind.bullet_type_ext.ballistic_scatter_min.unwrap_or(0),
                kind.bullet_type_ext
                    .ballistic_scatter_max
                    .unwrap_or(rules.combat_damage.ballistic_scatter),
            );
        }
    }
    (0, rules.combat_damage.ballistic_scatter)
}

// ---------------------------------------------------------------------------
// StraightTrajectory per-frame AI
// ---------------------------------------------------------------------------

#[cfg(feature = "phobos")]
impl Simulation {
    /// One `BulletClass::AI` visit for a bullet carrying a Phobos trajectory.
    /// Replaces the stock advance; the caller commits the returned detonations
    /// through the normal projectile boundary and retires the bullet.
    pub(super) fn step_phobos_trajectory(
        &mut self,
        bullet_id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> TrajectoryStep {
        let kind = self
            .projectiles
            .get(bullet_id)
            .and_then(|projectile| projectile.phobos_trajectory().map(|state| state.kind));
        match kind {
            Some(TrajectoryFlag::Straight) => {
                self.step_straight_trajectory(bullet_id, rules, overlay_registry)
            }
            Some(TrajectoryFlag::Bombard) => {
                self.step_bombard_trajectory(bullet_id, rules, overlay_registry)
            }
            Some(TrajectoryFlag::Parabola) => {
                self.step_parabola_trajectory(bullet_id, rules, overlay_registry)
            }
            None => TrajectoryStep {
                expired: true,
                ..TrajectoryStep::default()
            },
        }
    }

    fn step_straight_trajectory(
        &mut self,
        bullet_id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> TrajectoryStep {
        let mut step = TrajectoryStep::default();
        let _frame = self.session.binary_frame() as i32;
        let type_name = {
            let Some(projectile) = self.projectiles.get(bullet_id) else {
                step.expired = true;
                return step;
            };
            let Some(type_id) = projectile.payload().projectile_type() else {
                step.expired = true;
                return step;
            };
            self.interner.resolve(type_id).to_owned()
        };
        let Some(kind) = rules.projectile(&type_name) else {
            step.expired = true;
            return step;
        };
        let t = &kind.phobos_trajectory.straight;

        // `OnAI`'s wait arm: `BulletPrepareCheck`.
        let wait = self
            .projectiles
            .get(bullet_id)
            .and_then(|projectile| projectile.phobos_trajectory())
            .map(|state| state.wait_one_frame)
            .unwrap_or(0);
        if wait != 0 {
            if wait == 2 {
                let target = self
                    .projectiles
                    .get(bullet_id)
                    .map(|projectile| projectile.target());
                if let Some(ProjectileTarget::Entity(id)) = target
                    && let Some(entity) = self.substrate.entities.get(id)
                {
                    let coords = crate::sim::movement::ground_pose::object_get_coords(
                        entity,
                        self.resolved_terrain.as_ref(),
                    );
                    if let Some(state) = self
                        .projectiles
                        .get_mut(bullet_id)
                        .and_then(|projectile| projectile.phobos_trajectory_mut())
                    {
                        state.last_target_coord =
                            ProjectileCoord::new(coords.x, coords.y, coords.z);
                        state.wait_one_frame = 1;
                    }
                    return step;
                }
            }
            if let Some(state) = self
                .projectiles
                .get_mut(bullet_id)
                .and_then(|projectile| projectile.phobos_trajectory_mut())
            {
                state.wait_one_frame = 0;
            }
            self.prepare_phobos_open_fire(
                bullet_id,
                rules,
                t,
                kind.phobos_trajectory.speed,
                kind.inaccurate,
            );
        }

        // `BulletDetonatePreCheck` (`StraightTrajectory.cpp:510-537`).
        if self.trajectory_pre_detonate_check(bullet_id, &kind.phobos_trajectory, rules) {
            return self.trajectory_detonate(bullet_id, rules, overlay_registry, &mut step);
        }

        // `BulletDetonateVelocityCheck` (`:540-610`).
        self.trajectory_detonate_velocity_check(bullet_id, rules, overlay_registry);

        // `PassWithDetonateAt` (`:739-760`).
        if t.pass_detonate {
            self.trajectory_pass_detonate(bullet_id, rules, overlay_registry, &mut step);
        }
        // `PrepareForDetonateAt` (`:763-976`).
        if self
            .projectiles
            .get(bullet_id)
            .and_then(|projectile| projectile.phobos_trajectory())
            .is_some_and(|state| state.proximity_impact != 0)
            && t.proximity_radius > 0
        {
            self.trajectory_proximity(bullet_id, rules, overlay_registry, &mut step);
        }
        // `PassAndConfineAtHeight` (`:1022-1056`).
        if kind.phobos_trajectory.speed < 256.0
            && t.confine_at_height > 0
            && self.trajectory_confine_at_height(bullet_id, rules, &kind.phobos_trajectory)
        {
            return self.trajectory_detonate(bullet_id, rules, overlay_registry, &mut step);
        }
        // `BulletDetonateLastCheck` (`:613-651`).
        self.trajectory_last_check(bullet_id, rules, &mut step);

        // The stock body's motion: velocity with gravity cancelled
        // (`OnAIVelocity`) and the target/techno checks skipped
        // (`OnAITargetCoordCheck` / `OnAITechnoCheck`).
        self.trajectory_move(
            TrajectoryFlag::Straight,
            bullet_id,
            rules,
            overlay_registry,
            &mut step,
        );
        step
    }
}

// ---------------------------------------------------------------------------
// Helpers shared with the per-frame AI
// ---------------------------------------------------------------------------

fn sub(a: ProjectileCoord, b: ProjectileCoord) -> ProjectileCoord {
    ProjectileCoord::new(
        a.x.wrapping_sub(b.x),
        a.y.wrapping_sub(b.y),
        a.z.wrapping_sub(b.z),
    )
}

fn add(a: ProjectileCoord, b: ProjectileCoord) -> ProjectileCoord {
    ProjectileCoord::new(
        a.x.wrapping_add(b.x),
        a.y.wrapping_add(b.y),
        a.z.wrapping_add(b.z),
    )
}

fn scale(a: ProjectileCoord, by: i32) -> ProjectileCoord {
    ProjectileCoord::new(
        a.x.wrapping_mul(by),
        a.y.wrapping_mul(by),
        a.z.wrapping_mul(by),
    )
}

/// `CoordStruct::MagnitudeSquared`, taken in `i64` so an overflowing native
/// product does not panic; the lead solve compares relative values only.
fn mag_sq(a: ProjectileCoord) -> i64 {
    let x = i64::from(a.x);
    let y = i64::from(a.y);
    let z = i64::from(a.z);
    x * x + y * y + z * z
}

fn cross(a: ProjectileCoord, b: ProjectileCoord) -> ProjectileCoord {
    ProjectileCoord::new(
        a.y.wrapping_mul(b.z).wrapping_sub(a.z.wrapping_mul(b.y)),
        a.z.wrapping_mul(b.x).wrapping_sub(a.x.wrapping_mul(b.z)),
        a.x.wrapping_mul(b.y).wrapping_sub(a.y.wrapping_mul(b.x)),
    )
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `CalculateDisperseBurst`'s Rodrigues rotation, shared by Straight
/// (`StraightTrajectory.cpp:457-471`), Bombard (`BombardTrajectory.cpp:
/// 408-446`) and Parabola (`ParabolaTrajectory.cpp:276-310`).
#[allow(clippy::too_many_arguments)]
pub(super) fn burst_velocity(
    velocity: [f64; 3],
    rotate_angle: f64,
    axis_of_rotation: [i32; 3],
    rotate_coord: f64,
    mirror: bool,
    current_burst: i32,
    count_of_burst: i32,
    divisor: f64,
) -> [f64; 3] {
    let (sin_a, cos_a) = sin_cos(rotate_angle);
    let axis = [
        f64::from(axis_of_rotation[0]) * cos_a + f64::from(axis_of_rotation[1]) * sin_a,
        f64::from(axis_of_rotation[0]) * sin_a - f64::from(axis_of_rotation[1]) * cos_a,
        f64::from(axis_of_rotation[2]),
    ];
    let axis_len_sq = dot(axis, axis);
    if axis_len_sq.abs() <= 1e-10 {
        return velocity;
    }
    let len = axis_len_sq.sqrt();
    let mut axis = [axis[0] / len, axis[1] / len, axis[2] / len];
    let extra = if mirror {
        if current_burst % 2 == 1 {
            axis = [-axis[0], -axis[1], -axis[2]];
        }
        std::f64::consts::PI
            * (rotate_coord
                * ((current_burst / 2) as f64 / (f64::from(count_of_burst) - 1.0) - 0.5))
            / divisor
    } else {
        std::f64::consts::PI
            * (rotate_coord * (current_burst as f64 / (f64::from(count_of_burst) - 1.0) - 0.5))
            / divisor
    };
    let (sin_r, cos_r) = sin_cos(extra);
    let k = (1.0 - cos_r) * dot(velocity, axis);
    let c = cross_f(velocity, axis);
    [
        velocity[0] * cos_r + axis[0] * k + c[0] * sin_r,
        velocity[1] * cos_r + axis[1] * k + c[1] * sin_r,
        velocity[2] * cos_r + axis[2] * k + c[2] * sin_r,
    ]
}

fn cross_f(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn distance(a: ProjectileCoord, b: ProjectileCoord) -> f64 {
    crate::util::native_x87::distance_3d_leptons([a.x, a.y, a.z], [b.x, b.y, b.z]) as f64
}

/// `Math::atan2` over the table (`0x004CAE30`), narrowed like the Math
/// wrapper does; the caller then takes the table sine and cosine.
fn atan2(y: i32, x: i32) -> f64 {
    let angle = crate::util::direction_tables::native_atan2_f32(
        NativeF32Bits::from_bits((y as f32).to_bits()),
        NativeF32Bits::from_bits((x as f32).to_bits()),
    );
    f64::from_bits(X87Chop53::store_f64(angle).expect("finite atan2").bits())
}

fn sin_cos(radians: f64) -> (f64, f64) {
    let table = crate::util::retail_trig::TrigTable::embedded();
    let angle =
        X87Chop53::load_f64(NativeF64Bits::from_bits(radians.to_bits())).expect("finite angle");
    let sin = f64::from_bits(
        X87Chop53::store_f64(table.sin_from_table(angle))
            .expect("finite sine")
            .bits(),
    );
    let cos = f64::from_bits(
        X87Chop53::store_f64(table.cos_from_table(angle))
            .expect("finite cosine")
            .bits(),
    );
    (sin, cos)
}

#[cfg(test)]
#[path = "phobos_trajectory_tests.rs"]
mod tests;
