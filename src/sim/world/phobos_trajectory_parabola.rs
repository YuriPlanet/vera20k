//! `ParabolaTrajectory` (`Ext/Bullet/Trajectories/ParabolaTrajectory.cpp`):
//! the six open-fire modes, the optional lead-time solve, the bounce ladder
//! and the detonation-angle/height/distance gates. The per-bullet fields live
//! in [`super::PhobosTrajectoryState`], shared with the other classes.
//!
//! ## RNG draws
//! - `PrepareForOpenFire`'s `Inaccurate=` scatter draws through the shared
//!   retail-table polar helper (`ParabolaTrajectory.cpp:248-256`).
//! - Everything else here is deterministic arithmetic.
//!
//! ## Residuals
//! - `BounceDetonate`'s extra warhead is the bullet's own warhead and damage,
//!   detonated through the shared projectile detonation boundary
//!   (`:882-887`); the native `DetonateAt` owner adds no other effect.
//!
//! ## Dependency rules
//! - Part of sim/; no render/ui/audio/net.

use super::*;
use crate::rules::phobos_trajectory::ParabolaFireMode;

impl Simulation {
    /// `ParabolaTrajectory::OnUnlimbo` (`ParabolaTrajectory.cpp:122-158`).
    pub(super) fn init_parabola_trajectory(
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
        let base = &kind.phobos_trajectory;
        let t = &base.parabola;
        let source_id = projectile.source_id();
        let target = projectile.target();

        let mut state = PhobosTrajectoryState::base(TrajectoryFlag::Parabola);
        state.last_target_coord = projectile.last_target_position();
        state.throw_height = if t.throw_height > 0 {
            t.throw_height
        } else {
            600
        };
        state.bounce_times = t.bounce_times;
        state.offset_coord = t.offset_coord;
        state.use_disperse_burst = t.use_disperse_burst;
        state.count_of_burst = weapon.burst;

        // Special case: set the target to the ground (`:131-139`).
        let mut reset_target = false;
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
        if t.detonation_distance <= -1 && target_is_foot {
            if let ProjectileTarget::Entity(id) = target
                && let Some(entity) = self.substrate.entities.get(id)
            {
                let coords = crate::sim::movement::ground_pose::object_get_coords(
                    entity,
                    self.resolved_terrain.as_ref(),
                );
                if self.parabola_cell_exists(ProjectileCoord::new(coords.x, coords.y, coords.z)) {
                    let rx = (coords.x / 256) as u16;
                    let ry = (coords.y / 256) as u16;
                    let cell_coords = crate::sim::projectile::cell_ground_coord(
                        self.resolved_terrain.as_ref(),
                        rx,
                        ry,
                    );
                    state.last_target_coord = cell_coords;
                    if let Some(projectile) = self.projectiles.get_mut(bullet_id) {
                        projectile.set_target(ProjectileTarget::Cell { rx, ry });
                    }
                    reset_target = true;
                }
            }
        }

        if let Some(firer) = self.substrate.entities.get(source_id) {
            state.current_burst = firer.seldom.weapon_burst.index();
            if t.mirror_coord && state.current_burst % 2 == 1 {
                state.offset_coord[1] = state.offset_coord[1].wrapping_neg();
            }
        }

        let wait = t.lead_time_calculate && target_is_foot && !reset_target;
        if wait {
            state.wait_one_frame = 2;
        }
        if let Some(projectile) = self.projectiles.get_mut(bullet_id) {
            projectile.set_velocity(ProjectileVelocity::new(0, 0, 0));
            projectile.set_phobos_trajectory(Some(state));
        }
        if !wait {
            self.parabola_prepare_open_fire(bullet_id, rules);
        }
    }

    /// `ParabolaTrajectory::OnAI` (`:160-175`) and the stock motion tail.
    pub(super) fn step_parabola_trajectory(
        &mut self,
        bullet_id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> TrajectoryStep {
        let mut step = TrajectoryStep::default();

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
                    if let Some(projectile) = self.projectiles.get_mut(bullet_id) {
                        projectile.set_last_target_position(ProjectileCoord::new(
                            coords.x, coords.y, coords.z,
                        ));
                        if let Some(state) = projectile.phobos_trajectory_mut() {
                            state.wait_one_frame = 1;
                        }
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
            self.parabola_prepare_open_fire(bullet_id, rules);
        }

        if self.parabola_pre_detonate_check(bullet_id, rules) {
            self.parabola_finish(bullet_id, rules, overlay_registry, &mut step);
            return step;
        }

        let position = match self.projectiles.get(bullet_id) {
            Some(projectile) => projectile.position(),
            None => {
                step.expired = true;
                return step;
            }
        };
        if !self.parabola_cell_exists(position) {
            self.parabola_finish(bullet_id, rules, overlay_registry, &mut step);
            return step;
        }

        let bounce = self
            .projectiles
            .get(bullet_id)
            .and_then(|projectile| projectile.phobos_trajectory())
            .is_some_and(|state| state.should_bounce);
        if bounce && self.parabola_velocity_after_bounce(bullet_id, rules, &mut step) {
            self.parabola_finish(bullet_id, rules, overlay_registry, &mut step);
            return step;
        }

        if self.parabola_last_check(bullet_id, rules, overlay_registry, bounce) {
            self.parabola_finish(bullet_id, rules, overlay_registry, &mut step);
            return step;
        }

        self.trajectory_move(
            TrajectoryFlag::Parabola,
            bullet_id,
            rules,
            overlay_registry,
            &mut step,
        );
        step
    }

    /// `PrepareForOpenFire` (`:218-311`).
    fn parabola_prepare_open_fire(&mut self, bullet_id: u64, rules: &RuleSet) {
        let Some(projectile) = self.projectiles.get(bullet_id) else {
            return;
        };
        let Some((base, inaccurate)) = parabola_kind(rules, projectile, self) else {
            return;
        };
        let t = &base.parabola;
        let Some(mut state) = projectile.phobos_trajectory().cloned() else {
            return;
        };
        let target_ref = projectile.target();
        let source_coords = projectile.launch_origin();
        let location = projectile.position();
        // `LastTargetCoord` is the bullet field the lead solve compares
        // against; `TargetCoords` is the mutable aim kept in the state.
        let last_target = projectile.last_target_position();

        let target_coords = match target_ref {
            ProjectileTarget::Entity(id) => self
                .substrate
                .entities
                .get(id)
                .map(|entity| {
                    crate::sim::movement::ground_pose::object_get_coords(
                        entity,
                        self.resolved_terrain.as_ref(),
                    )
                })
                .map(|coords| ProjectileCoord::new(coords.x, coords.y, coords.z)),
            ProjectileTarget::Cell { rx, ry } => Some(crate::sim::projectile::cell_ground_coord(
                self.resolved_terrain.as_ref(),
                rx,
                ry,
            )),
            _ => None,
        };
        let mut lead = t.lead_time_calculate && target_coords.is_some();
        let mut the_target = if lead {
            target_coords.unwrap_or(last_target)
        } else {
            state.last_target_coord
        };
        let the_source = if lead { location } else { source_coords };
        lead &= the_target != last_target;

        let rotate_angle = if !t.lead_time_calculate
            && the_target == the_source
            && let Some(owner) = self.substrate.entities.get(projectile.source_id())
        {
            let owner_coords = crate::sim::movement::ground_pose::object_get_coords(
                owner,
                self.resolved_terrain.as_ref(),
            );
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

        if state.offset_coord != [0; 3] {
            let (sin_a, cos_a) = sin_cos(rotate_angle);
            let [ox, oy, oz] = state.offset_coord;
            the_target.x = the_target
                .x
                .wrapping_add((f64::from(ox) * cos_a + f64::from(oy) * sin_a) as i32);
            the_target.y = the_target
                .y
                .wrapping_add((f64::from(ox) * sin_a - f64::from(oy) * cos_a) as i32);
            the_target.z = the_target.z.wrapping_add(oz);
        }

        if inaccurate {
            let offset_mult = 0.0004 * distance(the_source, the_target);
            let (min, max) = match self.projectiles.get(bullet_id) {
                Some(projectile) => ballistic_scatter_bounds(rules, projectile, self),
                None => (0, 0),
            };
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
        state.last_target_coord = the_target;

        let gravity = self.parabola_gravity(bullet_id, rules);
        if gravity <= 1e-10 {
            if let Some(projectile) = self.projectiles.get_mut(bullet_id) {
                projectile.set_velocity(ProjectileVelocity::new(0, 0, 0));
                state.should_detonate = true;
                projectile.set_phobos_trajectory(Some(state));
            }
            return;
        }

        let mut velocity = [0.0f64; 3];
        if lead {
            self.parabola_velocity_lead_time(
                bullet_id,
                t,
                base.speed,
                the_source,
                the_target,
                gravity,
                &mut state,
                &mut velocity,
            );
        } else {
            self.parabola_velocity_right_now(
                t,
                base.speed,
                the_target,
                the_source,
                gravity,
                &mut state,
                &mut velocity,
            );
        }

        if !state.use_disperse_burst && t.rotate_coord.abs() > 1e-10 && state.count_of_burst > 1 {
            velocity = burst_velocity(
                velocity,
                rotate_angle,
                t.axis_of_rotation,
                t.rotate_coord,
                t.mirror_coord,
                state.current_burst,
                state.count_of_burst,
                180.0,
            );
        }

        if let Some(projectile) = self.projectiles.get_mut(bullet_id) {
            projectile.set_velocity(ProjectileVelocity::from_native(
                velocity.map(|value| NativeF64Bits::from_bits(value.to_bits())),
            ));
            projectile.set_phobos_trajectory(Some(state));
        }
    }

    /// `CalculateBulletVelocityRightNow` (`:544-683`).
    #[allow(clippy::too_many_arguments)]
    fn parabola_velocity_right_now(
        &self,
        t: &crate::rules::phobos_trajectory::ParabolaTrajectoryRules,
        speed: f64,
        target_coords: ProjectileCoord,
        source_coords: ProjectileCoord,
        gravity: f64,
        state: &mut PhobosTrajectoryState,
        velocity: &mut [f64; 3],
    ) {
        let distance_coords = sub(target_coords, source_coords);
        let distance = magnitude3(distance_coords);
        let horizontal = magnitude2(distance_coords);
        if distance <= 1e-10 {
            *velocity = [0.0, 0.0, 0.0];
            state.should_detonate = true;
            return;
        }
        match t.open_fire_mode {
            ParabolaFireMode::Height => {
                let source_height = f64::from(source_coords.z);
                let target_height = target_coords.z;
                let max_height = if distance_coords.z > 0 {
                    f64::from(state.throw_height) + f64::from(target_height)
                } else {
                    f64::from(state.throw_height) + source_height
                };
                velocity[2] = (2.0 * gravity * (max_height - source_height)).sqrt();
                let meet_time = ((2.0 * (max_height - source_height)) / gravity).sqrt()
                    + ((2.0 * (max_height - f64::from(target_height))) / gravity).sqrt();
                if meet_time.abs() > 1e-10 {
                    velocity[0] = f64::from(distance_coords.x) / meet_time;
                    velocity[1] = f64::from(distance_coords.y) / meet_time;
                }
            }
            ParabolaFireMode::Angle => {
                let radian = t.launch_angle * std::f64::consts::PI / 180.0;
                let solved =
                    solve_fixed_angle_velocity(horizontal, distance_coords.z, radian, gravity);
                if solved >= 0.0 {
                    velocity[2] = solved * sin(radian);
                    let mult = if horizontal > 1e-10 {
                        solved * cos(radian) / horizontal
                    } else {
                        1.0
                    };
                    velocity[0] = f64::from(distance_coords.x) * mult;
                    velocity[1] = f64::from(distance_coords.y) * mult;
                }
            }
            ParabolaFireMode::SpeedAndHeight => {
                let source_height = f64::from(source_coords.z);
                let target_height = target_coords.z;
                let max_height = if distance_coords.z > 0 {
                    f64::from(state.throw_height) + f64::from(target_height)
                } else {
                    f64::from(state.throw_height) + source_height
                };
                velocity[2] = (2.0 * gravity * (max_height - source_height)).sqrt();
                let mult = if horizontal > 1e-10 {
                    speed / horizontal
                } else {
                    1.0
                };
                velocity[0] = f64::from(distance_coords.x) * mult;
                velocity[1] = f64::from(distance_coords.y) * mult;
            }
            ParabolaFireMode::HeightAndAngle => {
                let source_height = f64::from(source_coords.z);
                let target_height = target_coords.z;
                let max_height = if distance_coords.z > 0 {
                    f64::from(state.throw_height) + f64::from(target_height)
                } else {
                    f64::from(state.throw_height) + source_height
                };
                velocity[2] = (2.0 * gravity * (max_height - source_height)).sqrt();
                let mut radian = t.launch_angle * std::f64::consts::PI / 180.0;
                radian = if radian >= std::f64::consts::FRAC_PI_2 || radian <= 1e-10 {
                    std::f64::consts::FRAC_PI_2 / 3.0
                } else {
                    radian
                };
                let mult = if horizontal.abs() > 1e-10 {
                    (velocity[2] / tan(radian)) / horizontal
                } else {
                    0.0
                };
                velocity[0] = f64::from(distance_coords.x) * mult;
                velocity[1] = f64::from(distance_coords.y) * mult;
            }
            ParabolaFireMode::SpeedAndAngle => {
                let mult = if horizontal > 1e-10 {
                    speed / horizontal
                } else {
                    1.0
                };
                velocity[0] = f64::from(distance_coords.x) * mult;
                velocity[1] = f64::from(distance_coords.y) * mult;
                let mut radian = t.launch_angle * std::f64::consts::PI / 180.0;
                radian = if radian >= std::f64::consts::FRAC_PI_2
                    || radian <= -std::f64::consts::FRAC_PI_2
                {
                    std::f64::consts::FRAC_PI_2 / 3.0
                } else {
                    radian
                };
                velocity[2] = speed * tan(radian);
            }
            ParabolaFireMode::Speed => {
                let mult = if horizontal > 1e-10 {
                    speed / horizontal
                } else {
                    1.0
                };
                velocity[0] = f64::from(distance_coords.x) * mult;
                velocity[1] = f64::from(distance_coords.y) * mult;
                velocity[2] =
                    f64::from(distance_coords.z) * mult + (gravity * horizontal) / (2.0 * speed);
            }
        }

        self.parabola_check_extra(t, speed, velocity, state);
        velocity[2] += gravity / 2.0;
    }

    /// `CalculateBulletVelocityLeadTime` (`:335-542`).
    #[allow(clippy::too_many_arguments)]
    fn parabola_velocity_lead_time(
        &mut self,
        bullet_id: u64,
        t: &crate::rules::phobos_trajectory::ParabolaTrajectoryRules,
        speed: f64,
        source_coords: ProjectileCoord,
        target_coords: ProjectileCoord,
        gravity: f64,
        state: &mut PhobosTrajectoryState,
        velocity: &mut [f64; 3],
    ) {
        let Some(projectile) = self.projectiles.get(bullet_id) else {
            return;
        };
        let live = match projectile.target() {
            ProjectileTarget::Entity(id) => {
                let Some(target) = self.substrate.entities.get(id) else {
                    return;
                };
                let coords = crate::sim::movement::ground_pose::object_get_coords(
                    target,
                    self.resolved_terrain.as_ref(),
                );
                ProjectileCoord::new(coords.x, coords.y, coords.z)
            }
            ProjectileTarget::Cell { rx, ry } => {
                crate::sim::projectile::cell_ground_coord(self.resolved_terrain.as_ref(), rx, ry)
            }
            _ => return,
        };
        let offset = sub(target_coords, live);
        let last = projectile.last_target_position();
        state.last_target_coord = target_coords;

        match t.open_fire_mode {
            ParabolaFireMode::Height => {
                let meet = search_fixed_height_meet_time(
                    source_coords,
                    live,
                    offset,
                    last,
                    f64::from(state.throw_height),
                    gravity,
                );
                state.last_target_coord = add(state.last_target_coord, predict(live, last, meet));
                let destination = sub(state.last_target_coord, source_coords);
                if meet <= 1e-10 || magnitude3(destination) <= 1e-10 {
                    self.parabola_unsolved_lead(
                        t,
                        speed,
                        source_coords,
                        live,
                        offset,
                        gravity,
                        state,
                        velocity,
                    );
                    return;
                }
                velocity[0] = f64::from(destination.x) / meet;
                velocity[1] = f64::from(destination.y) / meet;
                let source_height = f64::from(source_coords.z);
                let target_height = source_height + f64::from(destination.z);
                let max_height = if destination.z > 0 {
                    f64::from(state.throw_height) + target_height
                } else {
                    f64::from(state.throw_height) + source_height
                };
                velocity[2] = (2.0 * gravity * (max_height - source_height)).sqrt() + gravity / 2.0;
                self.parabola_check_extra(t, speed, velocity, state);
            }
            ParabolaFireMode::Angle => {
                let mut radian = t.launch_angle * std::f64::consts::PI / 180.0;
                radian = if radian >= std::f64::consts::FRAC_PI_2
                    || radian <= -std::f64::consts::FRAC_PI_2
                {
                    std::f64::consts::FRAC_PI_2 / 3.0
                } else {
                    radian
                };
                let meet = search_fixed_angle_meet_time(
                    source_coords,
                    live,
                    offset,
                    last,
                    radian,
                    gravity,
                );
                state.last_target_coord = add(state.last_target_coord, predict(live, last, meet));
                let destination = sub(state.last_target_coord, source_coords);
                if meet <= 1e-10 || magnitude3(destination) <= 1e-10 {
                    self.parabola_unsolved_lead(
                        t,
                        speed,
                        source_coords,
                        live,
                        offset,
                        gravity,
                        state,
                        velocity,
                    );
                    return;
                }
                velocity[0] = f64::from(destination.x) / meet;
                velocity[1] = f64::from(destination.y) / meet;
                let horizontal = magnitude2(destination);
                let horizontal_velocity = horizontal / meet;
                velocity[2] = horizontal_velocity * tan(radian) + gravity / 2.0;
                self.parabola_check_extra(t, speed, velocity, state);
            }
            ParabolaFireMode::SpeedAndHeight => {
                let meet = solve_fixed_speed_meet_time(source_coords, live, offset, last, speed);
                state.last_target_coord = add(state.last_target_coord, predict(live, last, meet));
                let destination = sub(state.last_target_coord, source_coords);
                if meet <= 1e-10 || magnitude3(destination) <= 1e-10 {
                    self.parabola_unsolved_lead(
                        t,
                        speed,
                        source_coords,
                        live,
                        offset,
                        gravity,
                        state,
                        velocity,
                    );
                    return;
                }
                let horizontal = magnitude2(destination);
                let mult = if horizontal > 1e-10 {
                    speed / horizontal
                } else {
                    1.0
                };
                velocity[0] = f64::from(destination.x) * mult;
                velocity[1] = f64::from(destination.y) * mult;
                let source_height = f64::from(source_coords.z);
                let target_height = source_height + f64::from(destination.z);
                let max_height = if destination.z > 0 {
                    f64::from(state.throw_height) + target_height
                } else {
                    f64::from(state.throw_height) + source_height
                };
                velocity[2] = (2.0 * gravity * (max_height - source_height)).sqrt() + gravity / 2.0;
                self.parabola_check_extra(t, speed, velocity, state);
            }
            ParabolaFireMode::HeightAndAngle => {
                let meet = search_fixed_height_meet_time(
                    source_coords,
                    live,
                    offset,
                    last,
                    f64::from(state.throw_height),
                    gravity,
                );
                state.last_target_coord = add(state.last_target_coord, predict(live, last, meet));
                let destination = sub(state.last_target_coord, source_coords);
                if meet <= 1e-10 || magnitude3(destination) <= 1e-10 {
                    self.parabola_unsolved_lead(
                        t,
                        speed,
                        source_coords,
                        live,
                        offset,
                        gravity,
                        state,
                        velocity,
                    );
                    return;
                }
                let source_height = f64::from(source_coords.z);
                let target_height = source_height + f64::from(destination.z);
                let max_height = if destination.z > 0 {
                    f64::from(state.throw_height) + target_height
                } else {
                    f64::from(state.throw_height) + source_height
                };
                velocity[2] = (2.0 * gravity * (max_height - source_height)).sqrt() + gravity / 2.0;
                let mut radian = t.launch_angle * std::f64::consts::PI / 180.0;
                radian = if radian >= std::f64::consts::FRAC_PI_2 || radian <= 1e-10 {
                    std::f64::consts::FRAC_PI_2 / 3.0
                } else {
                    radian
                };
                let horizontal = magnitude2(destination);
                let mult = if horizontal.abs() > 1e-10 {
                    (velocity[2] / tan(radian)) / horizontal
                } else {
                    0.0
                };
                velocity[0] = f64::from(destination.x) * mult;
                velocity[1] = f64::from(destination.y) * mult;
                self.parabola_check_extra(t, speed, velocity, state);
            }
            ParabolaFireMode::SpeedAndAngle => {
                let meet = solve_fixed_speed_meet_time(source_coords, live, offset, last, speed);
                state.last_target_coord = add(state.last_target_coord, predict(live, last, meet));
                let destination = sub(state.last_target_coord, source_coords);
                if meet <= 1e-10 || magnitude3(destination) <= 1e-10 {
                    self.parabola_unsolved_lead(
                        t,
                        speed,
                        source_coords,
                        live,
                        offset,
                        gravity,
                        state,
                        velocity,
                    );
                    return;
                }
                let horizontal = magnitude2(destination);
                let mult = if horizontal > 1e-10 {
                    speed / horizontal
                } else {
                    1.0
                };
                velocity[0] = f64::from(destination.x) * mult;
                velocity[1] = f64::from(destination.y) * mult;
                let horizontal_velocity = horizontal * mult;
                let mut radian = t.launch_angle * std::f64::consts::PI / 180.0;
                radian = if radian >= std::f64::consts::FRAC_PI_2
                    || radian <= -std::f64::consts::FRAC_PI_2
                {
                    std::f64::consts::FRAC_PI_2 / 3.0
                } else {
                    radian
                };
                velocity[2] = horizontal_velocity * tan(radian) + gravity / 2.0;
                self.parabola_check_extra(t, speed, velocity, state);
            }
            ParabolaFireMode::Speed => {
                let meet = solve_fixed_speed_meet_time(source_coords, live, offset, last, speed);
                state.last_target_coord = add(state.last_target_coord, predict(live, last, meet));
                let destination = sub(state.last_target_coord, source_coords);
                if meet <= 1e-10 || magnitude3(destination) <= 1e-10 {
                    self.parabola_unsolved_lead(
                        t,
                        speed,
                        source_coords,
                        live,
                        offset,
                        gravity,
                        state,
                        velocity,
                    );
                    return;
                }
                let horizontal = magnitude2(destination);
                let mult = if horizontal > 1e-10 {
                    speed / horizontal
                } else {
                    1.0
                };
                velocity[0] = f64::from(destination.x) * mult;
                velocity[1] = f64::from(destination.y) * mult;
                velocity[2] = f64::from(destination.z) * mult
                    + (gravity * horizontal) / (2.0 * speed)
                    + gravity / 2.0;
                self.parabola_check_extra(t, speed, velocity, state);
            }
        }
    }

    /// The unsolvable lead arm: restore the target coordinate and run the
    /// no-lead solve (`ParabolaTrajectory.cpp:537-542`).
    #[allow(clippy::too_many_arguments)]
    fn parabola_unsolved_lead(
        &self,
        t: &crate::rules::phobos_trajectory::ParabolaTrajectoryRules,
        speed: f64,
        source_coords: ProjectileCoord,
        live: ProjectileCoord,
        offset: ProjectileCoord,
        gravity: f64,
        state: &mut PhobosTrajectoryState,
        velocity: &mut [f64; 3],
    ) {
        let restored = add(live, offset);
        state.last_target_coord = restored;
        self.parabola_velocity_right_now(
            t,
            speed,
            restored,
            source_coords,
            gravity,
            state,
            velocity,
        );
    }

    /// `CheckIfNeedExtraCheck` (`:685-704`).
    fn parabola_check_extra(
        &self,
        t: &crate::rules::phobos_trajectory::ParabolaTrajectoryRules,
        speed: f64,
        velocity: &[f64; 3],
        state: &mut PhobosTrajectoryState,
    ) {
        state.need_extra_check = match t.open_fire_mode {
            ParabolaFireMode::Height
            | ParabolaFireMode::Angle
            | ParabolaFireMode::HeightAndAngle => {
                velocity[0] * velocity[0] + velocity[1] * velocity[1] > 65536.0
            }
            _ => speed > 256.0,
        };
    }

    /// `BulletDetonatePreCheck` (`:1025-1057`).
    fn parabola_pre_detonate_check(&self, bullet_id: u64, rules: &RuleSet) -> bool {
        let Some(projectile) = self.projectiles.get(bullet_id) else {
            return true;
        };
        let Some((base, _)) = parabola_kind(rules, projectile, self) else {
            return true;
        };
        let t = &base.parabola;
        let Some(state) = projectile.phobos_trajectory() else {
            return true;
        };
        if state.should_detonate {
            return true;
        }
        let position = projectile.position();
        let source = projectile.launch_origin();
        let velocity = velocity_f64(projectile.velocity());

        if t.detonation_height >= 0
            && velocity[2] < 1e-10
            && position.z.wrapping_sub(source.z) < t.detonation_height
        {
            return true;
        }
        if t.detonation_angle.abs() < 1e-10 {
            if velocity[2] < 1e-10 {
                return true;
            }
        } else if t.detonation_angle.abs() < 90.0 {
            let horizontal = (velocity[0] * velocity[0] + velocity[1] * velocity[1]).sqrt();
            if horizontal > 1e-10 {
                if (velocity[2] / horizontal)
                    < tan(t.detonation_angle * std::f64::consts::PI / 180.0)
                {
                    return true;
                }
            } else if t.detonation_angle > 1e-10 || velocity[2] < 1e-10 {
                return true;
            }
        }
        distance(state.last_target_coord, position) < f64::from(t.detonation_distance)
    }

    /// `CalculateBulletVelocityAfterBounce` (`:866-899`), returning whether
    /// the bullet detonates instead of bouncing.
    fn parabola_velocity_after_bounce(
        &mut self,
        bullet_id: u64,
        rules: &RuleSet,
        step: &mut TrajectoryStep,
    ) -> bool {
        let Some(projectile) = self.projectiles.get(bullet_id) else {
            return true;
        };
        let Some((base, _)) = parabola_kind(rules, projectile, self) else {
            return true;
        };
        let t = &base.parabola;
        let Some(mut state) = projectile.phobos_trajectory().cloned() else {
            return true;
        };
        let position = projectile.position();
        let cell = crate::sim::cell_rect::get_cellclass_fallback(
            self.resolved_terrain.as_ref(),
            position.x / 256,
            position.y / 256,
        );
        if cell.land_type() == crate::rules::terrain_rules::LandType::Water.as_index()
            && !t.bounce_on_water
        {
            return true;
        }

        state.bounce_times = state.bounce_times.wrapping_sub(1);
        state.should_bounce = false;

        let normal = self.parabola_ground_normal(bullet_id);
        let last = [
            f64::from_bits(state.last_velocity[0].bits()),
            f64::from_bits(state.last_velocity[1].bits()),
            f64::from_bits(state.last_velocity[2].bits()),
        ];
        let projection = dot(last, normal);
        let coefficient = t.bounce_coefficient;
        let velocity = [
            (last[0] - normal[0] * projection * 2.0) * coefficient,
            (last[1] - normal[1] * projection * 2.0) * coefficient,
            (last[2] - normal[2] * projection * 2.0) * coefficient,
        ];

        if t.bounce_detonate {
            let payload = projectile.payload();
            step.extra_detonations.push(ProjectileDetonation {
                projectile_id: bullet_id,
                source_id: projectile.source_id(),
                target: ProjectileTarget::None,
                impact: position,
                payload: ProjectilePayload::new(
                    payload.base_damage,
                    payload.warhead,
                    None,
                    payload.bright(),
                ),
                reason: ProjectileDetonationReason::PhobosTrajectory,
            });
        }

        if let Some(projectile) = self.projectiles.get_mut(bullet_id) {
            let damage = projectile.payload().base_damage;
            if damage != 0 {
                let scaled = (f64::from(damage) * t.bounce_attenuation) as i32;
                projectile.payload_mut().base_damage = if scaled != 0 {
                    scaled
                } else if damage > 0 {
                    1
                } else {
                    -1
                };
            }
            projectile.set_velocity(ProjectileVelocity::from_native(
                velocity.map(|value| NativeF64Bits::from_bits(value.to_bits())),
            ));
            projectile.set_phobos_trajectory(Some(state));
        }
        false
    }

    /// `GetGroundNormalVector` (`:901-1009`).
    fn parabola_ground_normal(&self, bullet_id: u64) -> [f64; 3] {
        let Some(projectile) = self.projectiles.get(bullet_id) else {
            return [0.0, 0.0, 1.0];
        };
        let position = projectile.position();
        let cell = crate::sim::cell_rect::get_cellclass_fallback(
            self.resolved_terrain.as_ref(),
            position.x / 256,
            position.y / 256,
        );
        let index = cell.slope_type();
        if index != 0 {
            let factor = if index <= 4 {
                (
                    0.3763770469559380854890894443664,
                    0.9264665771223091335116047861327,
                )
            } else if index <= 12 {
                (
                    0.3522530794922131411764879370407,
                    0.8670845033654477321267395373309,
                )
            } else {
                (
                    0.5333964609104418418483761938761,
                    0.6564879518897745745826168540013,
                )
            };
            return match index {
                1 => [-factor.0, 0.0, factor.1],
                2 => [0.0, -factor.0, factor.1],
                3 => [factor.0, 0.0, factor.1],
                4 => [0.0, factor.0, factor.1],
                5 | 9 | 13 => [-factor.0, -factor.0, factor.1],
                6 | 10 | 14 => [factor.0, -factor.0, factor.1],
                7 | 11 | 15 => [factor.0, factor.0, factor.1],
                8 | 12 | 16 => [-factor.0, factor.0, factor.1],
                _ => [0.0, 0.0, 1.0],
            };
        }

        let velocity = velocity_f64(projectile.velocity());
        let horizontal = (velocity[0] * velocity[0] + velocity[1] * velocity[1]).sqrt();
        let velocity = if horizontal > 362.1 {
            let scale = 362.1 / horizontal;
            [
                velocity[0] * scale,
                velocity[1] * scale,
                velocity[2] * scale,
            ]
        } else {
            velocity
        };
        let velocity_coords = [velocity[0] as i32, velocity[1] as i32, velocity[2] as i32];
        let level_height = crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS;
        let cell_height = i32::from(cell.signed_level()) * level_height;
        let bullet_height = position.z;
        let last_pos = ProjectileCoord::new(
            position.x.wrapping_sub(velocity_coords[0]),
            position.y.wrapping_sub(velocity_coords[1]),
            position.z.wrapping_sub(velocity_coords[2]),
        );
        let last_cell_height = self.trajectory_floor_height(last_pos);
        if bullet_height < cell_height && cell_height.wrapping_sub(last_cell_height) > 384 {
            let cell_x = position.x / 256;
            let cell_y = position.y / 256;
            let reverse_x = if velocity[0] > 0.0 { -1 } else { 1 };
            let reverse_y = if velocity[1] > 0.0 { -1 } else { 1 };
            let mut index = 0;
            if self.parabola_hit_cliff(
                cell_x.wrapping_add(reverse_x),
                cell_y,
                bullet_height,
                last_cell_height,
            ) {
                if !self.parabola_hit_cliff(
                    cell_x,
                    cell_y.wrapping_add(reverse_y),
                    bullet_height,
                    last_cell_height,
                ) {
                    if !self.parabola_hit_cliff(
                        cell_x.wrapping_sub(reverse_x),
                        cell_y.wrapping_add(reverse_y),
                        bullet_height,
                        last_cell_height,
                    ) {
                        return [0.0, f64::from(reverse_y), 0.0];
                    }
                    index = 2;
                }
            } else if self.parabola_hit_cliff(
                cell_x.wrapping_add(reverse_x),
                cell_y.wrapping_sub(reverse_y),
                bullet_height,
                last_cell_height,
            ) {
                if self.parabola_hit_cliff(
                    cell_x,
                    cell_y.wrapping_add(reverse_y),
                    bullet_height,
                    last_cell_height,
                ) {
                    index = 1;
                } else if !self.parabola_hit_cliff(
                    cell_x.wrapping_sub(reverse_x),
                    cell_y.wrapping_add(reverse_y),
                    bullet_height,
                    last_cell_height,
                ) {
                    index = 2;
                }
            } else if self.parabola_hit_cliff(
                cell_x,
                cell_y.wrapping_add(reverse_y),
                bullet_height,
                last_cell_height,
            ) {
                return [f64::from(reverse_x), 0.0, 0.0];
            } else if self.parabola_hit_cliff(
                cell_x.wrapping_sub(reverse_x),
                cell_y.wrapping_add(reverse_y),
                bullet_height,
                last_cell_height,
            ) {
                index = 1;
            }
            return if index == 1 {
                [
                    0.8944271909999158785636694674925 * f64::from(reverse_x),
                    0.4472135954999579392818347337463 * f64::from(reverse_y),
                    0.0,
                ]
            } else if index == 2 {
                [
                    0.4472135954999579392818347337463 * f64::from(reverse_x),
                    0.8944271909999158785636694674925 * f64::from(reverse_y),
                    0.0,
                ]
            } else {
                [
                    0.7071067811865475244008443621049 * f64::from(reverse_x),
                    0.7071067811865475244008443621049 * f64::from(reverse_y),
                    0.0,
                ]
            };
        }
        [0.0, 0.0, 1.0]
    }

    /// `MapClass::TryGetCellAt`'s non-null answer for the coordinate's cell:
    /// the shared dummy means the lookup found no real cell.
    fn parabola_cell_exists(&self, coord: ProjectileCoord) -> bool {
        matches!(
            crate::sim::cell_rect::get_cellclass_fallback(
                self.resolved_terrain.as_ref(),
                coord.x / 256,
                coord.y / 256,
            ),
            crate::sim::cell_rect::CellRef::Real(_)
        )
    }

    /// `CheckBulletHitCliff` (`:1011-1023`).
    fn parabola_hit_cliff(
        &self,
        x: i32,
        y: i32,
        bullet_height: i32,
        last_cell_height: i32,
    ) -> bool {
        let cell =
            crate::sim::cell_rect::get_cellclass_fallback(self.resolved_terrain.as_ref(), x, y);
        if matches!(cell, crate::sim::cell_rect::CellRef::Dummy { .. }) {
            return false;
        }
        let cell_height =
            i32::from(cell.signed_level()) * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS;
        bullet_height < cell_height && cell_height.wrapping_sub(last_cell_height) > 384
    }

    /// `BulletDetonateLastCheck` (`:1059-1117`).
    fn parabola_last_check(
        &mut self,
        bullet_id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        bounce: bool,
    ) -> bool {
        let Some(projectile) = self.projectiles.get(bullet_id) else {
            return true;
        };
        if parabola_kind(rules, projectile, self).is_none() {
            return true;
        }
        let Some(mut state) = projectile.phobos_trajectory().cloned() else {
            return true;
        };
        let gravity = self.parabola_gravity(bullet_id, rules);
        let mut velocity = velocity_f64(projectile.velocity());
        velocity[2] -= gravity;
        let location = projectile.position();
        let subject_to_walls = projectile_type_subject_to_walls(rules, self, projectile);

        if state.need_extra_check {
            let velocity_coords =
                ProjectileCoord::new(velocity[0] as i32, velocity[1] as i32, velocity[2] as i32);
            let future = add(location, velocity_coords);
            let cell_dist = (
                location.x / 256 - future.x / 256,
                location.y / 256 - future.y / 256,
            );
            let pace = (cell_dist.0.abs(), cell_dist.1.abs());
            let large = pace.0.max(pace.1).max(0);
            let step = if large != 0 {
                ProjectileCoord::new(
                    (f64::from(velocity_coords.x) / f64::from(large)) as i32,
                    (f64::from(velocity_coords.y) / f64::from(large)) as i32,
                    (f64::from(velocity_coords.z) / f64::from(large)) as i32,
                )
            } else {
                ProjectileCoord::new(0, 0, 0)
            };
            let mut current = add(location, step);
            for i in 1..=large {
                let cell_height = self.trajectory_floor_height(current);
                if current.z < cell_height {
                    if bounce {
                        return true;
                    }
                    state.last_velocity =
                        velocity.map(|value| NativeF64Bits::from_bits(value.to_bits()));
                    let factor = (f64::from(i) - 0.5) / f64::from(large);
                    let (velocity, state) = parabola_effectuate(velocity, state, factor);
                    if let Some(projectile) = self.projectiles.get_mut(bullet_id) {
                        projectile.set_velocity(ProjectileVelocity::from_native(
                            velocity.map(|value| NativeF64Bits::from_bits(value.to_bits())),
                        ));
                        projectile.set_phobos_trajectory(Some(state));
                    }
                    return false;
                }
                if subject_to_walls && self.trajectory_cell_wall(rules, overlay_registry, current) {
                    let factor = f64::from(i) / f64::from(large);
                    velocity = [
                        velocity[0] * factor,
                        velocity[1] * factor,
                        velocity[2] * factor,
                    ];
                    state.should_detonate = true;
                    if let Some(projectile) = self.projectiles.get_mut(bullet_id) {
                        projectile.set_velocity(ProjectileVelocity::from_native(
                            velocity.map(|value| NativeF64Bits::from_bits(value.to_bits())),
                        ));
                        projectile.set_phobos_trajectory(Some(state));
                    }
                    return false;
                }
                current = add(current, step);
            }
            if let Some(projectile) = self.projectiles.get_mut(bullet_id) {
                projectile.set_velocity(ProjectileVelocity::from_native(
                    velocity.map(|value| NativeF64Bits::from_bits(value.to_bits())),
                ));
                projectile.set_phobos_trajectory(Some(state));
            }
            return false;
        }

        let velocity_coords =
            ProjectileCoord::new(velocity[0] as i32, velocity[1] as i32, velocity[2] as i32);
        let future = add(location, velocity_coords);
        let cell_height = self.trajectory_floor_height(future);
        if cell_height < future.z {
            if let Some(projectile) = self.projectiles.get_mut(bullet_id) {
                projectile.set_velocity(ProjectileVelocity::from_native(
                    velocity.map(|value| NativeF64Bits::from_bits(value.to_bits())),
                ));
                projectile.set_phobos_trajectory(Some(state));
            }
            return false;
        }
        if bounce {
            return true;
        }
        state.last_velocity = velocity.map(|value| NativeF64Bits::from_bits(value.to_bits()));
        let factor = if velocity[2].abs() > 1e-10 {
            ((f64::from(location.z) - f64::from(cell_height)) / velocity[2]).abs()
        } else {
            0.0
        };
        let (velocity, state) = parabola_effectuate(velocity, state, factor);
        if let Some(projectile) = self.projectiles.get_mut(bullet_id) {
            projectile.set_velocity(ProjectileVelocity::from_native(
                velocity.map(|value| NativeF64Bits::from_bits(value.to_bits())),
            ));
            projectile.set_phobos_trajectory(Some(state));
        }
        false
    }

    /// `OnAIPreDetonate` (`:177-201`).
    fn parabola_pre_detonate(&mut self, bullet_id: u64, rules: &RuleSet) {
        let Some(projectile) = self.projectiles.get(bullet_id) else {
            return;
        };
        let Some((base, _)) = parabola_kind(rules, projectile, self) else {
            return;
        };
        let snap = base.parabola.target_snap_distance;
        let position = projectile.position();
        if snap > 0 {
            let coords = match projectile.target() {
                ProjectileTarget::Entity(id) => self.substrate.entities.get(id).map(|entity| {
                    crate::sim::movement::ground_pose::object_get_coords(
                        entity,
                        self.resolved_terrain.as_ref(),
                    )
                }),
                _ => None,
            }
            .map(|coords| ProjectileCoord::new(coords.x, coords.y, coords.z));
            let coords = coords.unwrap_or(position);
            if distance(coords, position) <= f64::from(snap) {
                if let Some(projectile) = self.projectiles.get_mut(bullet_id) {
                    projectile.set_position(coords);
                    if let Some(state) = projectile.phobos_trajectory_mut() {
                        state.snapped_to_target = true;
                    }
                }
                return;
            }
        }
        let cell_height = self.trajectory_floor_height(position);
        if position.z < cell_height
            && let Some(projectile) = self.projectiles.get_mut(bullet_id)
        {
            projectile.set_position(ProjectileCoord::new(position.x, position.y, cell_height));
        }
    }

    /// `OnAIPreDetonate` then the shared `0x467E53` ladder.
    pub(super) fn parabola_finish(
        &mut self,
        bullet_id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        step: &mut TrajectoryStep,
    ) {
        self.parabola_pre_detonate(bullet_id, rules);
        self.trajectory_ladder_push(bullet_id, rules, overlay_registry, step);
    }

    fn parabola_gravity(&self, bullet_id: u64, rules: &RuleSet) -> f64 {
        let Some(projectile) = self.projectiles.get(bullet_id) else {
            return 0.0;
        };
        let collision = projectile.collision();
        f64::from_bits(
            crate::sim::projectile::projectile_gravity(
                rules.general.gravity,
                collision.gravity(),
                collision.floater,
            )
            .bits(),
        )
    }

    fn trajectory_cell_wall(
        &self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        coord: ProjectileCoord,
    ) -> bool {
        let world = self.projectile_collision_world(rules, overlay_registry);
        world.cell_wall(coord)
    }
}

/// `ParabolaTrajectory::BulletDetonateEffectuate` (`:1119-1129`).
fn parabola_effectuate(
    mut velocity: [f64; 3],
    mut state: PhobosTrajectoryState,
    factor: f64,
) -> ([f64; 3], PhobosTrajectoryState) {
    if factor < 1.0 {
        velocity = [
            velocity[0] * factor,
            velocity[1] * factor,
            velocity[2] * factor,
        ];
    }
    if state.bounce_times > 0 {
        state.should_bounce = true;
    } else {
        state.should_detonate = true;
    }
    (velocity, state)
}

/// `pBullet->Type`'s Parabola data, when the bullet carries one.
fn parabola_kind<'a>(
    rules: &'a RuleSet,
    projectile: &Projectile,
    sim: &Simulation,
) -> Option<(
    &'a crate::rules::phobos_trajectory::PhobosTrajectoryRules,
    bool,
)> {
    let type_id = projectile.payload().projectile_type()?;
    let kind = rules.projectile(sim.interner.resolve(type_id))?;
    (kind.phobos_trajectory.flag == Some(TrajectoryFlag::Parabola))
        .then_some((&kind.phobos_trajectory, kind.inaccurate))
}

fn projectile_type_subject_to_walls(
    rules: &RuleSet,
    sim: &Simulation,
    projectile: &Projectile,
) -> bool {
    projectile
        .payload()
        .projectile_type()
        .and_then(|id| rules.projectile(sim.interner.resolve(id)))
        .is_some_and(|kind| kind.subject_to_walls)
}

fn velocity_f64(velocity: ProjectileVelocity) -> [f64; 3] {
    [
        f64::from_bits(velocity.x.bits()),
        f64::from_bits(velocity.y.bits()),
        f64::from_bits(velocity.z.bits()),
    ]
}

fn magnitude3(coord: ProjectileCoord) -> f64 {
    let x = f64::from(coord.x);
    let y = f64::from(coord.y);
    let z = f64::from(coord.z);
    (x * x + y * y + z * z).sqrt()
}

fn magnitude2(coord: ProjectileCoord) -> f64 {
    let x = f64::from(coord.x);
    let y = f64::from(coord.y);
    (x * x + y * y).sqrt()
}

fn sin(radians: f64) -> f64 {
    let (sin, _) = sin_cos(radians);
    sin
}

fn cos(radians: f64) -> f64 {
    let (_, cos) = sin_cos(radians);
    cos
}

fn tan(radians: f64) -> f64 {
    let (sin, cos) = sin_cos(radians);
    sin / cos
}

/// `CoordStruct * double`: each component truncates toward zero.
fn predict(target: ProjectileCoord, last: ProjectileCoord, time: f64) -> ProjectileCoord {
    ProjectileCoord::new(
        (f64::from(target.x.wrapping_sub(last.x)) * time) as i32,
        (f64::from(target.y.wrapping_sub(last.y)) * time) as i32,
        (f64::from(target.z.wrapping_sub(last.z)) * time) as i32,
    )
}

/// `SolveFixedAngleVelocity` (`:710-743`).
fn solve_fixed_angle_velocity(horizontal: f64, vertical: i32, radian: f64, gravity: f64) -> f64 {
    const EPSILON: f64 = 1e-6;
    if gravity <= EPSILON {
        return -1.0;
    }
    let cos_theta = cos(radian);
    if cos_theta.abs() < EPSILON {
        return -1.0;
    }
    let tan_theta = tan(radian);
    let trajectory_term = horizontal * tan_theta - f64::from(vertical);
    if trajectory_term <= EPSILON {
        return -1.0;
    }
    let velocity_squared =
        (gravity * horizontal * horizontal) / (2.0 * cos_theta * cos_theta * trajectory_term);
    if velocity_squared <= 0.0 || !velocity_squared.is_finite() {
        return -1.0;
    }
    velocity_squared.sqrt()
}

/// `SolveFixedSpeedMeetTime` (`:745-773`).
fn solve_fixed_speed_meet_time(
    source: ProjectileCoord,
    target: ProjectileCoord,
    offset: ProjectileCoord,
    last: ProjectileCoord,
    horizontal_speed: f64,
) -> f64 {
    let target_speed_x = f64::from(target.x.wrapping_sub(last.x));
    let target_speed_y = f64::from(target.y.wrapping_sub(last.y));
    let destination_x = f64::from(target.x.wrapping_add(offset.x).wrapping_sub(source.x));
    let destination_y = f64::from(target.y.wrapping_add(offset.y).wrapping_sub(source.y));
    let target_speed_sq = target_speed_x * target_speed_x + target_speed_y * target_speed_y;
    let destination_sq = destination_x * destination_x + destination_y * destination_y;
    let divisor = (target_speed_sq - horizontal_speed * horizontal_speed) * 2.0;
    let factor = 2.0 * (target_speed_x * destination_x + target_speed_y * destination_y);
    let delta = factor * factor - 2.0 * divisor * destination_sq;
    if delta >= 1e-10 {
        let time_p = (-factor + delta.sqrt()) / divisor;
        let time_m = (-factor - delta.sqrt()) / divisor;
        if time_m > 1e-10 && time_p > 1e-10 {
            return time_m.min(time_p);
        } else if time_m > 1e-10 {
            return time_m;
        } else if time_p > 1e-10 {
            return time_p;
        }
    }
    -1.0
}

/// `SearchFixedHeightMeetTime` (`:775-799`).
fn search_fixed_height_meet_time(
    source: ProjectileCoord,
    target: ProjectileCoord,
    offset: ProjectileCoord,
    last: ProjectileCoord,
    throw_height: f64,
    gravity: f64,
) -> f64 {
    let delta = 1e-5;
    let mut meet_time = (throw_height * 4.0) / gravity;
    for _ in 0..10 {
        let differential = check_fixed_height_equation(
            source,
            target,
            offset,
            last,
            meet_time,
            throw_height,
            gravity,
        );
        let d_differential = (check_fixed_height_equation(
            source,
            target,
            offset,
            last,
            meet_time + delta,
            throw_height,
            gravity,
        ) - differential)
            / delta;
        if d_differential.abs() < 1e-10 {
            return meet_time;
        }
        let difference = differential / d_differential;
        let meet_time_new = meet_time - difference;
        if difference.abs() < 1.0 {
            return meet_time_new;
        }
        meet_time = meet_time_new;
    }
    -1.0
}

/// `CheckFixedHeightEquation` (`:801-811`).
fn check_fixed_height_equation(
    source: ProjectileCoord,
    target: ProjectileCoord,
    offset: ProjectileCoord,
    last: ProjectileCoord,
    meet_time: f64,
    throw_height: f64,
    gravity: f64,
) -> f64 {
    let meet_height =
        (f64::from(target.z.wrapping_sub(last.z)) * meet_time) as i32 + target.z + offset.z;
    let max_height = if meet_height > source.z {
        throw_height + f64::from(meet_height)
    } else {
        throw_height + f64::from(source.z)
    };
    ((max_height - f64::from(source.z)) * 2.0 / gravity).sqrt()
        + ((max_height - f64::from(meet_height)) * 2.0 / gravity).sqrt()
        - meet_time
}

/// `SearchFixedAngleMeetTime` (`:813-837`).
fn search_fixed_angle_meet_time(
    source: ProjectileCoord,
    target: ProjectileCoord,
    offset: ProjectileCoord,
    last: ProjectileCoord,
    radian: f64,
    gravity: f64,
) -> f64 {
    let delta = 1e-5;
    let mut meet_time = 512.0 * sin(radian) / gravity;
    for _ in 0..10 {
        let differential =
            check_fixed_angle_equation(source, target, offset, last, meet_time, radian, gravity);
        let d_differential = (check_fixed_angle_equation(
            source,
            target,
            offset,
            last,
            meet_time + delta,
            radian,
            gravity,
        ) - differential)
            / delta;
        if d_differential.abs() < 1e-10 {
            return meet_time;
        }
        let difference = differential / d_differential;
        let meet_time_new = meet_time - difference;
        if difference.abs() < 1.0 {
            return meet_time_new;
        }
        meet_time = meet_time_new;
    }
    -1.0
}

/// `CheckFixedAngleEquation` (`:839-864`).
fn check_fixed_angle_equation(
    source: ProjectileCoord,
    target: ProjectileCoord,
    offset: ProjectileCoord,
    last: ProjectileCoord,
    meet_time: f64,
    radian: f64,
    gravity: f64,
) -> f64 {
    let distance = ProjectileCoord::new(
        ((f64::from(target.x.wrapping_sub(last.x)) * meet_time) as i32)
            .wrapping_add(target.x)
            .wrapping_add(offset.x)
            .wrapping_sub(source.x),
        ((f64::from(target.y.wrapping_sub(last.y)) * meet_time) as i32)
            .wrapping_add(target.y)
            .wrapping_add(offset.y)
            .wrapping_sub(source.y),
        ((f64::from(target.z.wrapping_sub(last.z)) * meet_time) as i32)
            .wrapping_add(target.z)
            .wrapping_add(offset.z)
            .wrapping_sub(source.z),
    );
    let horizontal = magnitude2(distance);
    let horizontal_velocity = horizontal / meet_time;
    let vertical_velocity = horizontal_velocity * tan(radian);
    let up_time = vertical_velocity / gravity;
    let max_height = 0.5 * vertical_velocity * up_time;
    let down_time = (2.0 * (max_height - f64::from(distance.z)) / gravity).sqrt();
    up_time + down_time - meet_time
}
