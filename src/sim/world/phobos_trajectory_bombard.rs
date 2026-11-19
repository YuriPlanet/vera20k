//! `BombardTrajectory` (`Ext/Bullet/Trajectories/BombardTrajectory.cpp`):
//! the lob to the turning point, the optional fall phase and the drop onto
//! the target. The per-bullet fields live in
//! [`super::PhobosTrajectoryState`], shared with the other classes.
//!
//! ## RNG draws
//! - `OnUnlimbo`: `RandomRanged(0, int(200 * FallPercentShift))`
//!   (`BombardTrajectory.cpp:120`).
//! - `CalculateMiddleCoords`: `RandomRanged(FallScatter_Min, Max)` and then
//!   either one `RandomDouble` (radial) or one `RandomRanged(0, 1)` (linear)
//!   (`:250-273`).
//! - `CalculateTargetCoords`'s `Inaccurate=` scatter draws through the shared
//!   retail-table polar helper.
//!
//! ## Residuals
//! - `BulletExt::LaserTrails` (not the stock `UseLineTrail` line trail) are
//!   not modeled on bullets; the reset at `:231-238`, `:556-564` belongs to
//!   that presentation owner.
//!
//! ## Dependency rules
//! - Part of sim/; no render/ui/audio/net.

use super::*;
use crate::rules::phobos_trajectory::BombardTrajectoryRules;

impl Simulation {
    /// `BombardTrajectory::OnUnlimbo` (`BombardTrajectory.cpp:115-144`).
    pub(super) fn init_bombard_trajectory(
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
        let t = &base.bombard;
        let target_coords = projectile.last_target_position();
        let source_id = projectile.source_id();
        let target = projectile.target();

        let mut state = PhobosTrajectoryState::base(TrajectoryFlag::Bombard);
        state.offset_coord = t.offset_coord;
        state.use_disperse_burst = t.use_disperse_burst;
        state.initial_target_coord = target_coords;
        state.last_target_coord = target_coords;
        state.bombard_height =
            NativeF64Bits::from_bits((t.height + f64::from(target_coords.z)).to_bits());
        // `FallPercent += RandomRanged(0, int(200 * FallPercentShift)) / 100.0`.
        let shift = sse_i32(200.0 * t.fall_percent_shift);
        let draw = self.scenario_rng.next_range_i32_inclusive(0, shift);
        state.bombard_fall_percent = NativeF64Bits::from_bits(
            (t.fall_percent - t.fall_percent_shift + f64::from(draw) / 100.0).to_bits(),
        );
        state.count_of_burst = weapon.burst;

        if let Some(firer) = self.substrate.entities.get(source_id) {
            state.current_burst = firer.weapon_burst.index();
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
        let wait = t.no_launch && t.lead_time_calculate && target_is_foot;
        if wait {
            state.wait_one_frame = 2;
        }
        if let Some(projectile) = self.projectiles.get_mut(bullet_id) {
            projectile.set_velocity(ProjectileVelocity::new(0, 0, 0));
            projectile.set_phobos_trajectory(Some(state));
        }
        if !wait {
            self.bombard_prepare_open_fire(bullet_id, rules);
        }
    }

    /// `BombardTrajectory::OnAI` (`BombardTrajectory.cpp:146-161`) and the
    /// stock motion tail.
    pub(super) fn step_bombard_trajectory(
        &mut self,
        bullet_id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> TrajectoryStep {
        let mut step = TrajectoryStep::default();

        // `BulletPrepareCheck` (`:448-468`).
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
            self.bombard_prepare_open_fire(bullet_id, rules);
        }

        if self.bombard_pre_detonate_check(bullet_id, rules) {
            self.bombard_finish(bullet_id, rules, overlay_registry, &mut step);
            return step;
        }

        self.bombard_velocity_change(bullet_id, rules);

        if self.bombard_remain_check(bullet_id, rules) {
            self.bombard_finish(bullet_id, rules, overlay_registry, &mut step);
            return step;
        }

        self.trajectory_move(
            TrajectoryFlag::Bombard,
            bullet_id,
            rules,
            overlay_registry,
            &mut step,
        );
        step
    }

    /// `BombardTrajectory::PrepareForOpenFire` (`:192-245`).
    fn bombard_prepare_open_fire(&mut self, bullet_id: u64, rules: &RuleSet) {
        let Some(projectile) = self.projectiles.get(bullet_id) else {
            return;
        };
        let Some((base, inaccurate)) = bombard_kind(rules, projectile, self) else {
            return;
        };
        let t = &base.bombard;
        let Some(mut state) = projectile.phobos_trajectory().cloned() else {
            return;
        };
        let source_id = projectile.source_id();
        let source_coords = projectile.launch_origin();
        let location = projectile.position();

        self.bombard_calculate_target_coords(
            bullet_id,
            rules,
            t,
            inaccurate,
            &mut state,
            source_id,
            source_coords,
        );

        let mut velocity = [0.0f64; 3];
        let mut position = location;

        if !t.no_launch {
            let middle = self.bombard_middle_coords(t, &state, source_coords);
            velocity = [
                f64::from(middle.x.wrapping_sub(source_coords.x)),
                f64::from(middle.y.wrapping_sub(source_coords.y)),
                f64::from(middle.z.wrapping_sub(source_coords.z)),
            ];
            velocity = normalize_to(velocity, base.speed);
            if !state.use_disperse_burst && t.rotate_coord.abs() > 1e-10 && state.count_of_burst > 1
            {
                velocity = burst_velocity(
                    velocity,
                    f64::from_bits(state.rotate_angle.bits()),
                    t.axis_of_rotation,
                    t.rotate_coord,
                    t.mirror_coord,
                    state.current_burst,
                    state.count_of_burst,
                    180.0,
                );
            }
            state.remaining_distance = state
                .remaining_distance
                .wrapping_add((distance(middle, source_coords) + base.speed) as i32);
            position = middle;
        } else {
            state.is_falling = true;
            let middle;
            if !t.free_fall_on_target {
                middle = self.bombard_middle_coords(t, &state, source_coords);
                velocity = [
                    f64::from(state.last_target_coord.x.wrapping_sub(middle.x)),
                    f64::from(state.last_target_coord.y.wrapping_sub(middle.y)),
                    f64::from(state.last_target_coord.z.wrapping_sub(middle.z)),
                ];
                velocity = normalize_to(velocity, t.fall_speed);
                if !state.use_disperse_burst
                    && t.rotate_coord.abs() > 1e-10
                    && state.count_of_burst > 1
                {
                    velocity = burst_velocity(
                        velocity,
                        f64::from_bits(state.rotate_angle.bits()),
                        t.axis_of_rotation,
                        t.rotate_coord,
                        t.mirror_coord,
                        state.current_burst,
                        state.count_of_burst,
                        90.0,
                    );
                }
                state.remaining_distance = state.remaining_distance.wrapping_add(
                    (distance(state.last_target_coord, middle) + t.fall_speed) as i32,
                );
            } else {
                middle = ProjectileCoord::new(
                    state.last_target_coord.x,
                    state.last_target_coord.y,
                    sse_i32(f64::from_bits(state.bombard_height.bits())),
                );
            }
            position = middle;
            self.bombard_spawn_turning_point_anims(rules, t, bullet_id, source_id, middle);
        }

        if let Some(projectile) = self.projectiles.get_mut(bullet_id) {
            projectile.set_position(position);
            projectile.set_velocity(ProjectileVelocity::from_native(
                velocity.map(|value| NativeF64Bits::from_bits(value.to_bits())),
            ));
            // `BombardTrajectory.cpp:233..236`: the no-launch opening move
            // starts every trail at this middle point.
            if t.no_launch {
                projectile.reset_phobos_laser_trail_locations([position.x, position.y, position.z]);
            }
            projectile.set_phobos_trajectory(Some(state));
        }
    }

    /// `AnimExt::CreateRandomAnim` at the turning point
    /// (`BombardTrajectory.cpp:241-243`, `:565-569`): one `RandomRanged` pick
    /// from the resolvable names, the firer House (or the retained
    /// `BulletExt::FirerHouse`) and no owner object.
    fn bombard_spawn_turning_point_anims(
        &mut self,
        rules: &RuleSet,
        t: &BombardTrajectoryRules,
        bullet_id: u64,
        source_id: u64,
        location: ProjectileCoord,
    ) {
        if t.turning_point_anims.is_empty() {
            return;
        }
        let Some(name) = self.resolve_random_anim_name(rules, &t.turning_point_anims) else {
            return;
        };
        let house = self
            .substrate
            .entities
            .get(source_id)
            .map(|entity| entity.owner())
            .or_else(|| {
                self.projectiles
                    .get(bullet_id)
                    .and_then(|projectile| projectile.phobos_firer_house())
            });
        let Some(house) = house else {
            return;
        };
        let type_name = self.interner.intern(name);
        // `CreateRandomAnim(TurningPointAnims, middle, pBullet->Owner,
        // pOwner, true)`: the bullet owner is the invoker, not an owned
        // object.
        let _ = self.spawn_owned_anim(
            rules,
            type_name,
            location.into(),
            house,
            None,
            (source_id != 0).then_some(source_id),
        );
    }

    /// `CalculateTargetCoords` (`:283-323`): optional lead, coordinate-system
    /// orientation, fixed offset and the `Inaccurate=` scatter.
    #[allow(clippy::too_many_arguments)]
    fn bombard_calculate_target_coords(
        &mut self,
        bullet_id: u64,
        rules: &RuleSet,
        t: &crate::rules::phobos_trajectory::BombardTrajectoryRules,
        inaccurate: bool,
        state: &mut PhobosTrajectoryState,
        source_id: u64,
        source_coords: ProjectileCoord,
    ) {
        let mut target_coords = state.last_target_coord;
        if t.no_launch {
            let lead = self.bombard_calculate_lead_time(bullet_id, rules, t, state);
            target_coords = add(target_coords, lead);
        }
        state.last_target_coord = target_coords;

        let rotate_angle = if !t.lead_time_calculate
            && target_coords == source_coords
            && let Some(owner) = self.substrate.entities.get(source_id)
        {
            let owner_coords = crate::sim::movement::ground_pose::object_get_coords(
                owner,
                self.resolved_terrain.as_ref(),
            );
            atan2(
                target_coords.y.wrapping_sub(owner_coords.y),
                target_coords.x.wrapping_sub(owner_coords.x),
            )
        } else {
            atan2(
                target_coords.y.wrapping_sub(source_coords.y),
                target_coords.x.wrapping_sub(source_coords.x),
            )
        };
        state.rotate_angle = NativeF64Bits::from_bits(rotate_angle.to_bits());

        if state.offset_coord != [0; 3] {
            let (sin_a, cos_a) = sin_cos(rotate_angle);
            let [ox, oy, oz] = state.offset_coord;
            target_coords.x = target_coords
                .x
                .wrapping_add(sse_i32(f64::from(ox) * cos_a + f64::from(oy) * sin_a));
            target_coords.y = target_coords
                .y
                .wrapping_add(sse_i32(f64::from(ox) * sin_a - f64::from(oy) * cos_a));
            target_coords.z = target_coords.z.wrapping_add(oz);
        }

        if inaccurate {
            let offset_mult = 0.0004 * distance(source_coords, target_coords);
            let (min, max) = match self.projectiles.get(bullet_id) {
                Some(projectile) => ballistic_scatter_bounds(rules, projectile, self),
                None => (0, 0),
            };
            let offset_min = (offset_mult * f64::from(min)) as i32;
            let offset_max = (offset_mult * f64::from(max)) as i32;
            let offset_distance = self
                .scenario_rng
                .next_range_i32_inclusive(offset_min, offset_max);
            target_coords = crate::sim::projectile::launch::random_coords_near(
                self.cell_grid(),
                target_coords,
                offset_distance,
                &mut self.scenario_rng,
            );
        }

        state.last_target_coord = target_coords;
    }

    /// `CalculateMiddleCoords` (`:247-281`).
    fn bombard_middle_coords(
        &mut self,
        t: &crate::rules::phobos_trajectory::BombardTrajectoryRules,
        state: &PhobosTrajectoryState,
        source_coords: ProjectileCoord,
    ) -> ProjectileCoord {
        let length = self
            .scenario_rng
            .next_range_i32_inclusive(t.fall_scatter_min, t.fall_scatter_max);
        let fall_percent = f64::from_bits(state.bombard_fall_percent.bits());
        let vector_x =
            f64::from(state.last_target_coord.x.wrapping_sub(source_coords.x)) * fall_percent;
        let vector_y =
            f64::from(state.last_target_coord.y.wrapping_sub(source_coords.y)) * fall_percent;
        let mut scatter_x = 0.0;
        let mut scatter_y = 0.0;
        if !t.fall_scatter_linear {
            let angle = self.scenario_rng.random_double() * std::f64::consts::TAU;
            let (sin_a, cos_a) = sin_cos(angle);
            scatter_x = f64::from(length) * cos_a;
            scatter_y = f64::from(length) * sin_a;
        } else {
            let module = (vector_x * vector_x + vector_y * vector_y).sqrt();
            scatter_x = vector_y / module * f64::from(length);
            scatter_y = -(vector_x / module * f64::from(length));
            if self.scenario_rng.next_range_i32_inclusive(0, 1) != 0 {
                scatter_x = -scatter_x;
                scatter_y = -scatter_y;
            }
        }
        ProjectileCoord::new(
            source_coords.x.wrapping_add(sse_i32(vector_x + scatter_x)),
            source_coords.y.wrapping_add(sse_i32(vector_y + scatter_y)),
            sse_i32(f64::from_bits(state.bombard_height.bits())),
        )
    }

    /// `CalculateBulletLeadTime` (`:325-406`).
    fn bombard_calculate_lead_time(
        &self,
        bullet_id: u64,
        rules: &RuleSet,
        t: &crate::rules::phobos_trajectory::BombardTrajectoryRules,
        state: &PhobosTrajectoryState,
    ) -> ProjectileCoord {
        let mut coords = ProjectileCoord::new(0, 0, 0);
        if !t.lead_time_calculate {
            return coords;
        }
        let Some(projectile) = self.projectiles.get(bullet_id) else {
            return coords;
        };
        let target_coords = match projectile.target() {
            ProjectileTarget::Entity(id) => {
                let Some(target) = self.substrate.entities.get(id) else {
                    return coords;
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
            _ => return coords,
        };
        // `LastTargetCoord`, not `TargetCoords` (`:338-343`).
        let last = projectile.last_target_position();
        if target_coords == last {
            return coords;
        }
        let source_coords = projectile.position();
        let extra = sub(target_coords, last);
        let target_source = sub(source_coords, target_coords);
        let last_source = sub(source_coords, last);
        let height = f64::from_bits(state.bombard_height.bits());
        let mut travel_time = 0i32;

        if t.free_fall_on_target {
            let gravity = self.bombard_gravity(bullet_id, rules);
            travel_time +=
                sse_i32(((2.0 * (height - f64::from(target_coords.z))) / gravity).sqrt());
            coords = add(coords, scale(extra, travel_time.wrapping_add(1)));
        } else if t.no_launch {
            travel_time += sse_i32((height - f64::from(target_coords.z)) / t.fall_speed);
            coords = add(coords, scale(extra, travel_time.wrapping_add(1)));
        } else {
            let distance_sq = mag_sq(target_source);
            let target_speed_sq = mag_sq(extra);
            let target_speed = (target_speed_sq as f64).sqrt();
            let cross = cross(target_source, last_source);
            let vertical_sq = mag_sq(cross) as f64 / target_speed_sq as f64;
            let horizon_sq = distance_sq as f64 - vertical_sq;
            let horizon = horizon_sq.sqrt();
            let straight_sq = t.fall_speed * t.fall_speed;
            let base_factor = straight_sq - target_speed_sq as f64;
            let square_factor = base_factor * vertical_sq + straight_sq * horizon_sq;
            if square_factor > 1e-10 {
                let minus = -(horizon * target_speed);
                if base_factor.abs() < 1e-10 {
                    travel_time = if horizon.abs() > 1e-10 {
                        sse_i32(distance_sq as f64 / (2.0 * horizon * target_speed)).wrapping_add(1)
                    } else {
                        0
                    };
                } else {
                    let travel_m = sse_i32((minus - square_factor.sqrt()) / base_factor);
                    let travel_p = sse_i32((minus + square_factor.sqrt()) / base_factor);
                    if travel_m > 0 && travel_p > 0 {
                        travel_time = travel_m.min(travel_p);
                    } else if travel_m > 0 {
                        travel_time = travel_m;
                    } else if travel_p > 0 {
                        travel_time = travel_p;
                    }
                    travel_time =
                        travel_time.wrapping_add(if mag_sq(target_source) < mag_sq(last_source) {
                            1
                        } else {
                            2
                        });
                }
                coords = add(coords, scale(extra, travel_time));
            }
        }
        coords
    }

    /// `BulletDetonatePreCheck` (`:470-495`).
    fn bombard_pre_detonate_check(&self, bullet_id: u64, rules: &RuleSet) -> bool {
        let Some(projectile) = self.projectiles.get(bullet_id) else {
            return true;
        };
        let Some((base, _)) = bombard_kind(rules, projectile, self) else {
            return true;
        };
        let t = &base.bombard;
        let Some(state) = projectile.phobos_trajectory() else {
            return true;
        };
        let position = projectile.position();
        let source = projectile.launch_origin();

        if distance(state.last_target_coord, position) < f64::from(t.detonation_distance) {
            return true;
        }
        if t.detonation_height >= 0 {
            let above = position.z.wrapping_sub(source.z);
            if t.early_detonation && above > t.detonation_height {
                return true;
            }
            if state.is_falling && above < t.detonation_height {
                return true;
            }
        }
        if (t.subject_to_ground || (state.is_falling && t.free_fall_on_target))
            && self.trajectory_floor_height(position) >= position.z.wrapping_add(15)
        {
            return true;
        }
        false
    }

    /// `BulletVelocityChange` (`:514-587`).
    fn bombard_velocity_change(&mut self, bullet_id: u64, rules: &RuleSet) {
        let Some(projectile) = self.projectiles.get(bullet_id) else {
            return;
        };
        let Some((base, _)) = bombard_kind(rules, projectile, self) else {
            return;
        };
        let t = &base.bombard;
        let Some(mut state) = projectile.phobos_trajectory().cloned() else {
            return;
        };
        let location = projectile.position();
        let target_ref = projectile.target();
        let source_id = projectile.source_id();
        let mut velocity = velocity_f64(projectile.velocity());
        let mut position = location;

        if !state.is_falling {
            state.remaining_distance = state.remaining_distance.wrapping_sub(sse_i32(base.speed));
            if state.remaining_distance < sse_i32(base.speed) {
                if state.to_falling {
                    state.is_falling = true;
                    state.remaining_distance = 1;
                    if !t.free_fall_on_target {
                        if t.lead_time_calculate
                            && let ProjectileTarget::Entity(id) = target_ref
                            && let Some(target) = self.substrate.entities.get(id)
                        {
                            let coords = crate::sim::movement::ground_pose::object_get_coords(
                                target,
                                self.resolved_terrain.as_ref(),
                            );
                            let target_coords = ProjectileCoord::new(coords.x, coords.y, coords.z);
                            let lead =
                                self.bombard_calculate_lead_time(bullet_id, rules, t, &state);
                            state.last_target_coord = add(
                                add(
                                    state.last_target_coord,
                                    sub(target_coords, state.initial_target_coord),
                                ),
                                lead,
                            );
                        }
                        position = location;
                        velocity = [
                            f64::from(state.last_target_coord.x.wrapping_sub(position.x)),
                            f64::from(state.last_target_coord.y.wrapping_sub(position.y)),
                            f64::from(state.last_target_coord.z.wrapping_sub(position.z)),
                        ];
                        velocity = normalize_to(velocity, t.fall_speed);
                        if !state.use_disperse_burst
                            && t.rotate_coord.abs() > 1e-10
                            && state.count_of_burst > 1
                        {
                            velocity = burst_velocity(
                                velocity,
                                f64::from_bits(state.rotate_angle.bits()),
                                t.axis_of_rotation,
                                t.rotate_coord,
                                t.mirror_coord,
                                state.current_burst,
                                state.count_of_burst,
                                90.0,
                            );
                        }
                        state.remaining_distance = state.remaining_distance.wrapping_add(
                            (distance(state.last_target_coord, position) + t.fall_speed) as i32,
                        );
                    } else {
                        if t.lead_time_calculate
                            && let ProjectileTarget::Entity(id) = target_ref
                            && let Some(target) = self.substrate.entities.get(id)
                        {
                            let coords = crate::sim::movement::ground_pose::object_get_coords(
                                target,
                                self.resolved_terrain.as_ref(),
                            );
                            let target_coords = ProjectileCoord::new(coords.x, coords.y, coords.z);
                            let lead =
                                self.bombard_calculate_lead_time(bullet_id, rules, t, &state);
                            state.last_target_coord = add(
                                add(
                                    state.last_target_coord,
                                    sub(target_coords, state.initial_target_coord),
                                ),
                                lead,
                            );
                        }
                        position = ProjectileCoord::new(
                            state.last_target_coord.x,
                            state.last_target_coord.y,
                            location.z,
                        );
                        velocity = [0.0, 0.0, 0.0];
                    }
                    // `BombardTrajectory.cpp:558..561`: the turn to the fall
                    // phase restarts every trail at this middle point.
                    if let Some(projectile) = self.projectiles.get_mut(bullet_id) {
                        projectile.reset_phobos_laser_trail_locations([
                            position.x, position.y, position.z,
                        ]);
                    }
                    self.bombard_spawn_turning_point_anims(
                        rules, t, bullet_id, source_id, position,
                    );
                } else {
                    state.to_falling = true;
                    if t.lead_time_calculate
                        && let ProjectileTarget::Entity(id) = target_ref
                        && let Some(target) = self.substrate.entities.get(id)
                    {
                        let coords = crate::sim::movement::ground_pose::object_get_coords(
                            target,
                            self.resolved_terrain.as_ref(),
                        );
                        if let Some(projectile) = self.projectiles.get_mut(bullet_id) {
                            projectile.set_last_target_position(ProjectileCoord::new(
                                coords.x, coords.y, coords.z,
                            ));
                        }
                    }
                    let factor = f64::from(state.remaining_distance) / base.speed;
                    velocity = [
                        velocity[0] * factor,
                        velocity[1] * factor,
                        velocity[2] * factor,
                    ];
                }
            }
        } else if t.free_fall_on_target {
            velocity[2] -= self.bombard_gravity(bullet_id, rules);
        }

        if let Some(projectile) = self.projectiles.get_mut(bullet_id) {
            projectile.set_position(position);
            projectile.set_velocity(ProjectileVelocity::from_native(
                velocity.map(|value| NativeF64Bits::from_bits(value.to_bits())),
            ));
            projectile.set_phobos_trajectory(Some(state));
        }
    }

    /// `BulletDetonateRemainCheck` (`:497-512`); only the falling,
    /// non-free-fall arm calls it.
    fn bombard_remain_check(&mut self, bullet_id: u64, rules: &RuleSet) -> bool {
        let Some(projectile) = self.projectiles.get(bullet_id) else {
            return true;
        };
        let Some((base, _)) = bombard_kind(rules, projectile, self) else {
            return true;
        };
        let t = &base.bombard;
        let Some(mut state) = projectile.phobos_trajectory().cloned() else {
            return true;
        };
        if !(state.is_falling && !t.free_fall_on_target) {
            return false;
        }
        state.remaining_distance = state.remaining_distance.wrapping_sub(sse_i32(t.fall_speed));
        if state.remaining_distance < 0 {
            if let Some(projectile) = self.projectiles.get_mut(bullet_id) {
                projectile.set_phobos_trajectory(Some(state));
            }
            return true;
        }
        let mut velocity = velocity_f64(projectile.velocity());
        if (state.remaining_distance as f64) < t.fall_speed {
            let factor = f64::from(state.remaining_distance) / t.fall_speed;
            velocity = [
                velocity[0] * factor,
                velocity[1] * factor,
                velocity[2] * factor,
            ];
            state.remaining_distance = 0;
        }
        if let Some(projectile) = self.projectiles.get_mut(bullet_id) {
            projectile.set_velocity(ProjectileVelocity::from_native(
                velocity.map(|value| NativeF64Bits::from_bits(value.to_bits())),
            ));
            projectile.set_phobos_trajectory(Some(state));
        }
        false
    }

    /// `OnAIPreDetonate` (`:163-175`).
    fn bombard_pre_detonate(&mut self, bullet_id: u64, rules: &RuleSet) {
        let Some(projectile) = self.projectiles.get(bullet_id) else {
            return;
        };
        let Some((base, _)) = bombard_kind(rules, projectile, self) else {
            return;
        };
        let t = &base.bombard;
        let coords = match projectile.target() {
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
            _ => None,
        };
        let coords = coords.unwrap_or_else(|| projectile.position());
        if distance(coords, projectile.position()) <= f64::from(t.target_snap_distance)
            && let Some(projectile) = self.projectiles.get_mut(bullet_id)
        {
            projectile.set_position(coords);
            if let Some(state) = projectile.phobos_trajectory_mut() {
                state.snapped_to_target = true;
            }
        }
    }

    /// `OnAIPreDetonate` then the shared `0x467E53` ladder.
    pub(super) fn bombard_finish(
        &mut self,
        bullet_id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        step: &mut TrajectoryStep,
    ) {
        self.bombard_pre_detonate(bullet_id, rules);
        self.trajectory_ladder_push(bullet_id, rules, overlay_registry, step);
    }

    fn bombard_gravity(&self, bullet_id: u64, rules: &RuleSet) -> f64 {
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
}

/// `static_cast<int>` in Phobos' `/arch:SSE2` build: `cvttsd2si` truncates
/// toward zero and returns `INT_MIN` for NaN, infinity and out-of-range values.
fn sse_i32(value: f64) -> i32 {
    if value >= -2_147_483_648.0 && value < 2_147_483_648.0 {
        value as i32
    } else {
        i32::MIN
    }
}

#[cfg(all(test, tests_sim_world_p_s))]
mod tests {
    use super::sse_i32;

    /// `static_cast<int>` in the SSE2 build (`cvttsd2si`).
    #[test]
    fn sse_cast_returns_the_integer_indefinite_out_of_range() {
        assert_eq!(sse_i32(f64::NAN), i32::MIN);
        assert_eq!(sse_i32(f64::INFINITY), i32::MIN);
        assert_eq!(sse_i32(-f64::INFINITY), i32::MIN);
        assert_eq!(sse_i32(2_147_483_648.0), i32::MIN);
        assert_eq!(sse_i32(-2_147_483_648.0), i32::MIN);
        assert_eq!(sse_i32(-4.9), -4);
        assert_eq!(sse_i32(4.9), 4);
    }
}

/// `pBullet->Type`'s Bombard data, when the bullet carries one.
fn bombard_kind<'a>(
    rules: &'a RuleSet,
    projectile: &Projectile,
    sim: &Simulation,
) -> Option<(
    &'a crate::rules::phobos_trajectory::PhobosTrajectoryRules,
    bool,
)> {
    let type_id = projectile.payload().projectile_type()?;
    let kind = rules.projectile(sim.interner.resolve(type_id))?;
    (kind.phobos_trajectory.flag == Some(TrajectoryFlag::Bombard))
        .then_some((&kind.phobos_trajectory, kind.inaccurate))
}

fn velocity_f64(velocity: ProjectileVelocity) -> [f64; 3] {
    [
        f64::from_bits(velocity.x.bits()),
        f64::from_bits(velocity.y.bits()),
        f64::from_bits(velocity.z.bits()),
    ]
}

fn normalize_to(velocity: [f64; 3], speed: f64) -> [f64; 3] {
    let length =
        (velocity[0] * velocity[0] + velocity[1] * velocity[1] + velocity[2] * velocity[2]).sqrt();
    if length > 1e-10 {
        let scale = speed / length;
        [
            velocity[0] * scale,
            velocity[1] * scale,
            velocity[2] * scale,
        ]
    } else {
        [0.0, 0.0, 0.0]
    }
}
