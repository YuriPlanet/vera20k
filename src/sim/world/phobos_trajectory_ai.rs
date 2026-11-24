//! `StraightTrajectory`'s per-frame arms, split from
//! [`super::phobos_trajectory`]'s state and Unlimbo half.

use super::*;
use crate::map::entities::EntityCategory;
use crate::sim::projectile::ProjectileCollisionMotion;

impl Simulation {
    /// `BulletDetonatePreCheck` (`StraightTrajectory.cpp:510-537`).
    pub(super) fn trajectory_pre_detonate_check(
        &mut self,
        bullet_id: u64,
        base: &crate::rules::phobos_trajectory::PhobosTrajectoryRules,
        rules: &RuleSet,
    ) -> bool {
        let t = &base.straight;
        let speed = base.speed as i32;
        let Some(projectile) = self.projectiles.get(bullet_id) else {
            return true;
        };
        let Some(state) = projectile.phobos_trajectory() else {
            return true;
        };
        if state.extra_check.is_some() {
            return true;
        }
        let remaining = state.remaining_distance.wrapping_sub(speed);
        let position = projectile.position();
        let target = state.last_target_coord;
        let detonation_distance = state.detonation_distance;
        if let Some(state) = self
            .projectiles
            .get_mut(bullet_id)
            .and_then(|projectile| projectile.phobos_trajectory_mut())
        {
            state.remaining_distance = remaining;
        }
        if remaining < 0 {
            return true;
        }
        if !t.pass_through && distance(target, position) < f64::from(detonation_distance) {
            return true;
        }
        if t.subject_to_ground
            && self.trajectory_floor_height(position) >= position.z.wrapping_add(16)
        {
            return true;
        }
        let _ = rules;
        !self.trajectory_inside_map(position)
    }

    /// `MapClass::GetCellFloorHeight` through the projectile ground owner.
    pub(super) fn trajectory_floor_height(&self, coord: ProjectileCoord) -> i32 {
        let dummy = self.effective_shared_cell_dummy();
        crate::sim::projectile::projectile_ground_z(self.resolved_terrain.as_ref(), &dummy, coord)
    }

    pub(super) fn trajectory_inside_map(&self, coord: ProjectileCoord) -> bool {
        self.map_coordinates_legal(coord.x, coord.y)
    }

    /// `OnAIPreDetonate` (`StraightTrajectory.cpp:209-241`) then the standard
    /// detonation handoff.
    pub(super) fn trajectory_detonate(
        &mut self,
        bullet_id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        step: &mut TrajectoryStep,
    ) -> TrajectoryStep {
        let Some(projectile) = self.projectiles.get(bullet_id) else {
            return std::mem::take(step);
        };
        let Some(t) = trajectory_rules(rules, projectile, self) else {
            return std::mem::take(step);
        };
        let mut location = projectile.position();
        let target_ref = projectile.target();
        let pass_through = t.pass_through;
        let pass_local = t.pass_detonate_local;
        let snap_distance = t.target_snap_distance;
        let source_origin = projectile.launch_origin();

        // `pBullet->Health = GetTheTrueDamage(pBullet->Health, pBullet, pTechno, true)`.
        let damage = {
            let projectile = self.projectiles.get(bullet_id).expect("retained bullet");
            let state = projectile.phobos_trajectory().expect("retained trajectory");
            true_damage(
                projectile.payload().base_damage,
                state,
                t.straight_rules,
                None,
                true,
                source_origin,
                projectile.position(),
                self,
            )
        };
        if let Some(projectile) = self.projectiles.get_mut(bullet_id) {
            projectile.payload_mut().base_damage = damage;
        }
        if pass_local {
            location.z = self.trajectory_floor_height(location);
            if let Some(projectile) = self.projectiles.get_mut(bullet_id) {
                projectile.set_position(location);
            }
        }
        if !pass_through
            && snap_distance > 0
            && let Some(target) = trajectory_target_coords(self, target_ref)
            && distance(location, target) <= f64::from(snap_distance)
        {
            location = target;
            if let Some(projectile) = self.projectiles.get_mut(bullet_id) {
                projectile.set_position(location);
                if let Some(state) = projectile.phobos_trajectory_mut() {
                    state.snapped_to_target = true;
                }
            }
        }
        if let Some(detonation) =
            self.trajectory_ladder_detonation(bullet_id, rules, overlay_registry)
        {
            step.detonations.push(detonation);
        }
        std::mem::take(step)
    }

    fn trajectory_ladder_detonation(
        &self,
        bullet_id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> Option<ProjectileDetonation> {
        let projectile = self.projectiles.get(bullet_id)?;
        let world = self.projectile_collision_world(rules, overlay_registry);
        Some(world.ladder_detonation(
            projectile,
            false,
            ProjectileDetonationReason::PhobosTrajectory,
        ))
    }

    /// The shared `0x467E53` impact for the Bombard and Parabola arms.
    pub(super) fn trajectory_ladder_push(
        &self,
        bullet_id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        step: &mut TrajectoryStep,
    ) {
        if let Some(detonation) =
            self.trajectory_ladder_detonation(bullet_id, rules, overlay_registry)
        {
            step.detonations.push(detonation);
        }
    }

    /// `BulletDetonateVelocityCheck` (`StraightTrajectory.cpp:540-610`).
    pub(super) fn trajectory_detonate_velocity_check(
        &mut self,
        bullet_id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) {
        let Some(projectile) = self.projectiles.get(bullet_id) else {
            return;
        };
        let Some(t) = trajectory_rules(rules, projectile, self) else {
            return;
        };
        let Some(state) = projectile.phobos_trajectory() else {
            return;
        };
        let speed = t.speed;
        let mut velocity_check = (state.remaining_distance as f64) < speed;
        let mut location_distance = f64::from(state.remaining_distance);
        let check_through = !t.through_building || !t.through_vehicles;
        let subject_to_walls = projectile
            .payload()
            .projectile_type()
            .and_then(|id| rules.projectile(self.interner.resolve(id)))
            .is_some_and(|kind| kind.subject_to_walls);
        let check_subject = t.subject_to_ground || subject_to_walls;
        let position = projectile.position();
        let velocity = projectile.velocity();
        let source_id = projectile.source_id();
        let target = projectile.target();
        let mut extra_check = state.extra_check;
        if speed < 256.0 {
            if check_through
                && let Some(id) = through_blocker(
                    self,
                    rules,
                    position,
                    source_id,
                    target,
                    t.through_vehicles,
                    t.through_building,
                )
            {
                extra_check = Some(id);
                location_distance = 0.0;
                velocity_check = true;
            }
        } else if check_through || check_subject {
            let step = velocity.integer_projection();
            let target_coord = add(position, step);
            let source_cell = (position.x / 256, position.y / 256);
            let target_cell = (target_coord.x / 256, target_coord.y / 256);
            let pace = (
                (source_cell.0 - target_cell.0).abs(),
                (source_cell.1 - target_cell.1).abs(),
            );
            let large = pace.0.max(pace.1).max(0);
            let step_coord = if large == 0 {
                ProjectileCoord::new(0, 0, 0)
            } else {
                ProjectileCoord::new(
                    (f64::from(target_coord.x.wrapping_sub(position.x)) / f64::from(large)) as i32,
                    (f64::from(target_coord.y.wrapping_sub(position.y)) / f64::from(large)) as i32,
                    (f64::from(target_coord.z.wrapping_sub(position.z)) / f64::from(large)) as i32,
                )
            };
            let mut cur = position;
            let mut cell_distance = location_distance;
            for _ in 0..large {
                let floor = self.trajectory_floor_height(cur);
                let wall = {
                    let world = self.projectile_collision_world(rules, overlay_registry);
                    world.cell_wall(cur)
                };
                let blocker = if check_through {
                    through_blocker(
                        self,
                        rules,
                        cur,
                        source_id,
                        target,
                        t.through_vehicles,
                        t.through_building,
                    )
                } else {
                    None
                };
                if (t.subject_to_ground && cur.z.wrapping_add(16) < floor)
                    || (subject_to_walls && wall)
                    || blocker.is_some()
                {
                    if let Some(id) = blocker {
                        extra_check = Some(id);
                    }
                    velocity_check = true;
                    cell_distance = distance(cur, position);
                    break;
                }
                cur = add(cur, step_coord);
            }
            location_distance = cell_distance;
        }
        if let Some(state) = self
            .projectiles
            .get_mut(bullet_id)
            .and_then(|projectile| projectile.phobos_trajectory_mut())
        {
            state.extra_check = extra_check;
        }
        if velocity_check {
            let Some(projectile) = self.projectiles.get(bullet_id) else {
                return;
            };
            let mut slowed = location_distance + 32.0;
            let mut velocity = projectile.velocity();
            if slowed < speed {
                velocity = scale_velocity(velocity, slowed / speed);
            }
            let _ = &mut slowed;
            if let Some(projectile) = self.projectiles.get_mut(bullet_id) {
                projectile.set_velocity(velocity);
                if let Some(state) = projectile.phobos_trajectory_mut() {
                    state.remaining_distance = 0;
                }
            }
        }
    }

    /// `PassWithDetonateAt` (`StraightTrajectory.cpp:739-760`).
    pub(super) fn trajectory_pass_detonate(
        &mut self,
        bullet_id: u64,
        rules: &RuleSet,
        _overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        step: &mut TrajectoryStep,
    ) {
        let frame = self.session.binary_frame() as i32;
        let Some(projectile) = self.projectiles.get(bullet_id) else {
            return;
        };
        let Some(t) = trajectory_rules(rules, projectile, self) else {
            return;
        };
        let Some(state) = projectile.phobos_trajectory() else {
            return;
        };
        if !state.pass_detonate_timer.expired(frame) {
            return;
        }
        let Some(warhead_name) = t.pass_detonate_warhead.as_deref() else {
            return;
        };
        let Some(warhead) = rules.warhead(warhead_name) else {
            return;
        };
        let warhead_ref = self.interner.intern(warhead_name);
        let delay = t.pass_detonate_delay.max(1);
        let mut coords = projectile.position();
        if t.pass_detonate_local {
            coords.z = self.trajectory_floor_height(coords);
        }
        let damage = true_damage(
            state.pass_detonate_damage,
            state,
            &t,
            None,
            false,
            projectile.launch_origin(),
            projectile.position(),
            self,
        );
        let source_id = projectile.source_id();
        if let Some(projectile) = self.projectiles.get_mut(bullet_id)
            && let Some(state) = projectile.phobos_trajectory_mut()
        {
            state.pass_detonate_timer = CdTimer::started(frame, delay);
        }
        step.extra_detonations.push(ProjectileDetonation {
            projectile_id: bullet_id,
            source_id,
            target: ProjectileTarget::None,
            impact: coords,
            payload: ProjectilePayload::new(damage, warhead_ref, None, warhead.bright),
            reason: ProjectileDetonationReason::PhobosTrajectory,
        });
        self.trajectory_calculate_new_damage(bullet_id, t.straight_rules);
    }

    /// `PrepareForDetonateAt` (`StraightTrajectory.cpp:763-976`).
    pub(super) fn trajectory_proximity(
        &mut self,
        bullet_id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        step: &mut TrajectoryStep,
    ) {
        let Some(projectile) = self.projectiles.get(bullet_id) else {
            return;
        };
        let Some(t) = trajectory_rules(rules, projectile, self) else {
            return;
        };
        let Some(warhead_name) = t.proximity_warhead.as_deref() else {
            return;
        };
        let Some(warhead) = rules.warhead(warhead_name) else {
            return;
        };
        let warhead_ref = self.interner.intern(warhead_name);
        let radius = t.proximity_radius;
        let Some(state) = projectile.phobos_trajectory().cloned() else {
            return;
        };
        if state.proximity_impact == 0 || radius <= 0 {
            return;
        }
        let position = projectile.position();
        let source_origin = projectile.launch_origin();
        let velocity = projectile.velocity();
        let target = projectile.target();
        let source_id = projectile.source_id();
        let owner_house = self
            .substrate
            .entities
            .get(source_id)
            .map(|entity| entity.owner());
        let cells = proximity_cells(projectile, radius, self.resolved_terrain.as_ref());
        let mut valid: Vec<u64> = Vec::new();
        let mut seen: Vec<u64> = Vec::new();
        for cell in &cells {
            let Some(occupancy) = self.substrate.occupancy.get(cell.0, cell.1) else {
                continue;
            };
            for occupant in
                occupancy.iter_layer(crate::sim::movement::locomotor::MovementLayer::Ground)
            {
                let id = occupant.entity_id;
                if seen.contains(&id) {
                    continue;
                }
                seen.push(id);
                if proximity_candidate(
                    self,
                    rules,
                    id,
                    source_id,
                    owner_house,
                    target,
                    source_origin,
                    position,
                    velocity,
                    radius,
                    t.proximity_allies,
                    false,
                ) {
                    valid.push(id);
                }
            }
        }
        if t.proximity_flight {
            let aircraft: Vec<u64> = self
                .substrate
                .entities
                .values()
                .filter(|entity| entity.category() == EntityCategory::Aircraft)
                .map(|entity| entity.stable_id())
                .collect();
            for id in aircraft {
                if !proximity_candidate(
                    self,
                    rules,
                    id,
                    source_id,
                    owner_house,
                    target,
                    source_origin,
                    position,
                    velocity,
                    radius,
                    t.proximity_allies,
                    true,
                ) {
                    continue;
                }
                valid.push(id);
            }
        }
        // `TheCasualty` bookkeeping (`:887-916`).
        let frame = self.session.binary_frame() as i32;
        let mut casualty = state.the_casualty.clone();
        if let Some(firer) = self
            .substrate
            .entities
            .get(source_id)
            .map(|entity| entity.stable_id())
        {
            casualty.insert(firer, 20);
        }
        let mut checked = Vec::new();
        for (id, remain) in &mut casualty {
            if *remain > 0 {
                *remain -= 1;
            } else {
                checked.push(*id);
            }
        }
        for id in checked {
            casualty.remove(&id);
        }
        let mut valid_targets = Vec::new();
        for id in valid {
            if !casualty.contains_key(&id) {
                valid_targets.push(id);
            }
            casualty.insert(id, 20);
        }
        if let Some(state) = self
            .projectiles
            .get_mut(bullet_id)
            .and_then(|projectile| projectile.phobos_trajectory_mut())
        {
            state.the_casualty = casualty;
        }
        let _ = frame;
        // Closest targets first when more than `ProximityImpact` remain.
        if state.proximity_impact > 0 && valid_targets.len() as i32 > state.proximity_impact {
            valid_targets.sort_by_key(|id| {
                (
                    self.trajectory_entity_distance_squared(*id, source_origin),
                    *id,
                )
            });
        }
        let mut impact = state.proximity_impact;
        for id in valid_targets {
            if Some(id) == state.extra_check {
                break;
            }
            if impact == 1 {
                if let Some(state) = self
                    .projectiles
                    .get_mut(bullet_id)
                    .and_then(|projectile| projectile.phobos_trajectory_mut())
                {
                    state.extra_check = Some(id);
                }
                break;
            }
            if !through_allows(self, rules, id, t.through_vehicles, t.through_building) {
                continue;
            }
            let Some(coords) = trajectory_target_coords(self, ProjectileTarget::Entity(id)) else {
                continue;
            };
            let current = self
                .projectiles
                .get(bullet_id)
                .and_then(|projectile| projectile.phobos_trajectory())
                .cloned();
            let Some(current) = current else {
                break;
            };
            let damage = true_damage(
                current.proximity_damage,
                &current,
                t.straight_rules,
                if t.proximity_medial {
                    None
                } else {
                    Some(coords)
                },
                false,
                source_origin,
                position,
                self,
            );
            if t.proximity_direct {
                let attacker_id = if source_id != 0 { source_id } else { 0 };
                step.direct_damage.push(EntityDamageEvent::direct_receiver(
                    id,
                    damage,
                    0,
                    attacker_id,
                    owner_house,
                    warhead_ref,
                    ReceiverCallFlags {
                        ignore_defenses: false,
                        arg6: false,
                    },
                ));
            } else if t.proximity_medial {
                let _ = coords;
                step.extra_detonations.push(ProjectileDetonation {
                    projectile_id: bullet_id,
                    source_id,
                    target: ProjectileTarget::None,
                    impact: position,
                    payload: ProjectilePayload::new(damage, warhead_ref, None, warhead.bright),
                    reason: ProjectileDetonationReason::PhobosTrajectory,
                });
            } else {
                step.extra_detonations.push(ProjectileDetonation {
                    projectile_id: bullet_id,
                    source_id,
                    target: ProjectileTarget::Entity(id),
                    impact: coords,
                    payload: ProjectilePayload::new(damage, warhead_ref, None, warhead.bright),
                    reason: ProjectileDetonationReason::PhobosTrajectory,
                });
            }
            self.trajectory_calculate_new_damage(bullet_id, t.straight_rules);
            if impact > 0 {
                impact -= 1;
                if let Some(state) = self
                    .projectiles
                    .get_mut(bullet_id)
                    .and_then(|projectile| projectile.phobos_trajectory_mut())
                {
                    state.proximity_impact = impact;
                }
            }
        }
        let _ = overlay_registry;
    }

    fn trajectory_entity_distance_squared(&self, id: u64, origin: ProjectileCoord) -> i64 {
        trajectory_target_coords(self, ProjectileTarget::Entity(id))
            .map(|coords| mag_sq(sub(coords, origin)))
            .unwrap_or(i64::MAX)
    }

    /// `PassAndConfineAtHeight` (`StraightTrajectory.cpp:1022-1056`).
    pub(super) fn trajectory_confine_at_height(
        &mut self,
        bullet_id: u64,
        rules: &RuleSet,
        base: &crate::rules::phobos_trajectory::PhobosTrajectoryRules,
    ) -> bool {
        let t = &base.straight;
        let speed = base.speed;
        let Some(projectile) = self.projectiles.get(bullet_id) else {
            return true;
        };
        let position = projectile.position();
        let velocity = projectile.velocity();
        let step = velocity.integer_projection();
        let future = add(position, step);
        let floor = self.trajectory_floor_height(future);
        let mut difference = floor.wrapping_sub(future.z);
        if cell_has_bridge(self, future) {
            let on_bridge =
                difference.wrapping_add(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS);
            if on_bridge.abs() < difference.abs() {
                difference = on_bridge;
            }
        }
        let subject_to_cliffs = projectile
            .payload()
            .projectile_type()
            .and_then(|id| rules.projectile(self.interner.resolve(id)))
            .is_some_and(|kind| kind.subject_to_cliffs);
        if difference.abs() < 384 || !subject_to_cliffs {
            let mut velocity = velocity;
            let z = f64::from_bits(velocity.z.bits())
                + f64::from(difference.wrapping_add(t.confine_at_height));
            velocity.z = NativeF64Bits::from_bits(z.to_bits());
            let length = libm::hypot(
                libm::hypot(
                    f64::from_bits(velocity.x.bits()),
                    f64::from_bits(velocity.y.bits()),
                ),
                z,
            );
            let (velocity, burned) = if length > 1e-10 {
                let scale = speed / length;
                (scale_velocity_f(velocity, scale), false)
            } else {
                (velocity, true)
            };
            if let Some(projectile) = self.projectiles.get_mut(bullet_id) {
                projectile.set_velocity(velocity);
                if burned && let Some(state) = projectile.phobos_trajectory_mut() {
                    state.remaining_distance = 0;
                }
            }
            !t.pass_detonate_local && burned
        } else {
            true
        }
    }

    /// `BulletDetonateLastCheck` (`StraightTrajectory.cpp:613-651`).
    pub(super) fn trajectory_last_check(
        &mut self,
        bullet_id: u64,
        rules: &RuleSet,
        step: &mut TrajectoryStep,
    ) {
        let Some(projectile) = self.projectiles.get(bullet_id) else {
            return;
        };
        let Some(t) = trajectory_rules(rules, projectile, self) else {
            return;
        };
        let Some(state) = projectile.phobos_trajectory().cloned() else {
            return;
        };
        let Some(extra) = state.extra_check else {
            return;
        };
        let Some(position) = trajectory_target_coords(self, ProjectileTarget::Entity(extra)) else {
            return;
        };
        let current = projectile.position();
        let distance = distance(position, current);
        let velocity = projectile.velocity();
        let magnitude = libm::hypot(
            libm::hypot(
                f64::from_bits(velocity.x.bits()),
                f64::from_bits(velocity.y.bits()),
            ),
            f64::from_bits(velocity.z.bits()),
        );
        let mut new_velocity = velocity;
        if magnitude.abs() > 1e-10 && distance < magnitude {
            new_velocity = scale_velocity(velocity, distance / magnitude);
        }
        let source_id = projectile.source_id();
        let owner_house = self
            .substrate
            .entities
            .get(source_id)
            .map(|entity| entity.owner());
        if let Some(projectile) = self.projectiles.get_mut(bullet_id) {
            projectile.set_target(ProjectileTarget::Entity(extra));
            projectile.set_last_target_position(position);
            projectile.set_velocity(new_velocity);
        }
        if state.proximity_impact != 0 {
            let Some(warhead_name) = t.proximity_warhead.as_deref() else {
                return;
            };
            let Some(warhead) = rules.warhead(warhead_name) else {
                return;
            };
            let warhead_ref = self.interner.intern(warhead_name);
            let origin = projectile_origin(self, bullet_id);
            let at = projectile_position(self, bullet_id);
            let damage = true_damage(
                state.proximity_damage,
                &state,
                t.straight_rules,
                if t.proximity_medial {
                    None
                } else {
                    Some(position)
                },
                false,
                origin,
                at,
                self,
            );
            if t.proximity_direct {
                step.direct_damage.push(EntityDamageEvent::direct_receiver(
                    extra,
                    damage,
                    0,
                    source_id,
                    owner_house,
                    warhead_ref,
                    ReceiverCallFlags {
                        ignore_defenses: false,
                        arg6: false,
                    },
                ));
            } else if t.proximity_medial {
                step.extra_detonations.push(ProjectileDetonation {
                    projectile_id: bullet_id,
                    source_id,
                    target: ProjectileTarget::None,
                    impact: at,
                    payload: ProjectilePayload::new(damage, warhead_ref, None, warhead.bright),
                    reason: ProjectileDetonationReason::PhobosTrajectory,
                });
            } else {
                step.extra_detonations.push(ProjectileDetonation {
                    projectile_id: bullet_id,
                    source_id,
                    target: ProjectileTarget::Entity(extra),
                    impact: position,
                    payload: ProjectilePayload::new(damage, warhead_ref, None, warhead.bright),
                    reason: ProjectileDetonationReason::PhobosTrajectory,
                });
            }
            self.trajectory_calculate_new_damage(bullet_id, t.straight_rules);
        }
    }

    /// The stock body's motion for a trajectory bullet: velocity with gravity
    /// cancelled and the target/techno checks skipped
    /// (`OnAIVelocity` / `OnAITargetCoordCheck` / `OnAITechnoCheck`).
    pub(super) fn trajectory_move(
        &mut self,
        kind: TrajectoryFlag,
        bullet_id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        step: &mut TrajectoryStep,
    ) {
        let Some(projectile) = self.projectiles.get(bullet_id) else {
            step.expired = true;
            return;
        };
        let position = projectile.position();
        let velocity = projectile.velocity();
        let degenerate = projectile
            .payload()
            .projectile_type()
            .and_then(|id| rules.projectile(self.interner.resolve(id)))
            .is_some_and(|kind| kind.degenerates);
        let candidate = add(position, velocity.integer_projection());
        let motion = ProjectileCollisionMotion::from_coordinate(candidate, velocity);
        let moved = {
            let world = self.projectile_collision_world(rules, overlay_registry);
            world.phobos_trajectory_motion(projectile, motion, velocity)
        };
        if moved.left_map {
            step.expired = true;
            return;
        }
        if moved.impact {
            if let Some(projectile) = self.projectiles.get_mut(bullet_id) {
                projectile.set_position(moved.candidate);
            }
            match kind {
                TrajectoryFlag::Straight => {
                    let done = self.trajectory_detonate(bullet_id, rules, overlay_registry, step);
                    step.detonations.extend(done.detonations);
                    step.extra_detonations.extend(done.extra_detonations);
                    step.direct_damage.extend(done.direct_damage);
                    step.expired |= done.expired;
                }
                TrajectoryFlag::Bombard => {
                    self.bombard_finish(bullet_id, rules, overlay_registry, step);
                }
                TrajectoryFlag::Parabola => {
                    self.parabola_finish(bullet_id, rules, overlay_registry, step);
                }
            }
            return;
        }
        if let Some(projectile) = self.projectiles.get_mut(bullet_id) {
            projectile.set_position(moved.candidate);
            projectile.set_velocity(moved.velocity);
            projectile.set_previous_cell((
                (moved.candidate.x / 256) as i16,
                (moved.candidate.y / 256) as i16,
            ));
            if degenerate && projectile.payload().base_damage > 5 {
                projectile.payload_mut().base_damage -= 1;
            }
        }
    }

    /// `CalculateNewDamage` (`StraightTrajectory.cpp:695-737`).
    pub(super) fn trajectory_calculate_new_damage(
        &mut self,
        bullet_id: u64,
        t: &StraightTrajectoryRules,
    ) {
        let ratio = t.count_attenuation;
        if ratio == 1.0 {
            return;
        }
        let scale = |value: i32| -> i32 {
            if ratio != 0.0 {
                let scaled = (f64::from(value) * ratio) as i32;
                if scaled != 0 { scaled } else { value.signum() }
            } else {
                0
            }
        };
        let Some(projectile) = self.projectiles.get_mut(bullet_id) else {
            return;
        };
        let base = projectile.payload().base_damage;
        let base = if ratio != 0.0 {
            if base != 0 { scale(base) } else { base }
        } else {
            0
        };
        projectile.payload_mut().base_damage = base;
        let Some(state) = projectile.phobos_trajectory_mut() else {
            return;
        };
        if ratio != 0.0 {
            if state.proximity_damage != 0 {
                state.proximity_damage = scale(state.proximity_damage);
            }
            if state.pass_detonate_damage != 0 {
                state.pass_detonate_damage = scale(state.pass_detonate_damage);
            }
        } else {
            state.proximity_damage = 0;
            state.pass_detonate_damage = 0;
        }
    }
}

/// The base `Trajectory.Speed` and the Straight type data.
#[derive(Clone, Copy)]
struct TrajRules<'a> {
    speed: f64,
    straight_rules: &'a StraightTrajectoryRules,
}

impl std::ops::Deref for TrajRules<'_> {
    type Target = StraightTrajectoryRules;

    fn deref(&self) -> &Self::Target {
        self.straight_rules
    }
}

/// `pBullet->Type`'s `Trajectory.Straight` data, when the bullet carries one.
fn trajectory_rules<'a>(
    rules: &'a RuleSet,
    projectile: &Projectile,
    sim: &Simulation,
) -> Option<TrajRules<'a>> {
    let type_id = projectile.payload().projectile_type()?;
    let kind = rules.projectile(sim.interner.resolve(type_id))?;
    (kind.phobos_trajectory.flag == Some(TrajectoryFlag::Straight)).then_some(TrajRules {
        speed: kind.phobos_trajectory.speed,
        straight_rules: &kind.phobos_trajectory.straight,
    })
}

fn projectile_origin(sim: &Simulation, id: u64) -> ProjectileCoord {
    sim.projectiles
        .get(id)
        .map(|projectile| projectile.launch_origin())
        .unwrap_or(ProjectileCoord::new(0, 0, 0))
}

fn projectile_position(sim: &Simulation, id: u64) -> ProjectileCoord {
    sim.projectiles
        .get(id)
        .map(|projectile| projectile.position())
        .unwrap_or(ProjectileCoord::new(0, 0, 0))
}

pub(super) fn trajectory_target_coords(
    sim: &Simulation,
    target: ProjectileTarget,
) -> Option<ProjectileCoord> {
    match target {
        ProjectileTarget::Entity(id) => {
            let entity = sim.substrate.entities.get(id)?;
            let coords = crate::sim::movement::ground_pose::object_get_coords(
                entity,
                sim.resolved_terrain.as_ref(),
            );
            Some(ProjectileCoord::new(coords.x, coords.y, coords.z))
        }
        _ => None,
    }
}

/// `GetTheTrueDamage` (`StraightTrajectory.cpp:978-999`) and
/// `GetExtraDamageMultiplier` (`:1001-1020`).
#[allow(clippy::too_many_arguments)]
fn true_damage(
    damage: i32,
    state: &PhobosTrajectoryState,
    t: &StraightTrajectoryRules,
    target_coords: Option<ProjectileCoord>,
    self_shot: bool,
    source_origin: ProjectileCoord,
    location: ProjectileCoord,
    _sim: &Simulation,
) -> i32 {
    if damage == 0 {
        return 0;
    }
    if t.edge_attenuation != 1.0 {
        let travelled = match target_coords {
            Some(coords) => distance_f64(coords, source_origin),
            None => distance_f64(location, source_origin),
        };
        let multiplier = extra_damage_multiplier(t, state, travelled);
        let calculated = if self_shot {
            f64::from(damage) * multiplier
        } else {
            f64::from(damage) * f64::from_bits(state.firepower_mult.bits()) * multiplier
        };
        let signal = if calculated > 0.0 {
            1
        } else if calculated < 0.0 {
            -1
        } else {
            0
        };
        let mut damage = calculated as i32;
        if damage == 0 && t.edge_attenuation > 0.0 {
            damage = signal;
        }
        return damage;
    }
    damage
}

fn extra_damage_multiplier(
    t: &StraightTrajectoryRules,
    state: &PhobosTrajectoryState,
    travelled: f64,
) -> f64 {
    const LEPTONS_PER_CELL: f64 = 256.0;
    if f64::from(state.attenuation_range) < travelled {
        return t.edge_attenuation;
    }
    let mut multiplier = 1.0;
    if travelled > LEPTONS_PER_CELL {
        multiplier += (t.edge_attenuation - 1.0)
            * ((travelled - LEPTONS_PER_CELL)
                / (f64::from(state.attenuation_range) - LEPTONS_PER_CELL));
    }
    multiplier
}

pub(super) fn distance_f64(a: ProjectileCoord, b: ProjectileCoord) -> f64 {
    let x = f64::from(a.x.wrapping_sub(b.x));
    let y = f64::from(a.y.wrapping_sub(b.y));
    let z = f64::from(a.z.wrapping_sub(b.z));
    libm::sqrt(x * x + y * y + z * z)
}

pub(super) fn scale_velocity(velocity: ProjectileVelocity, scale: f64) -> ProjectileVelocity {
    scale_velocity_f(velocity, scale)
}

pub(super) fn scale_velocity_f(velocity: ProjectileVelocity, scale: f64) -> ProjectileVelocity {
    let scale_bits = NativeF64Bits::from_bits(scale.to_bits());
    let component = |bits: NativeF64Bits| {
        NativeF64Bits::from_bits(
            (f64::from_bits(bits.bits()) * f64::from_bits(scale_bits.bits())).to_bits(),
        )
    };
    ProjectileVelocity::from_native([
        component(velocity.x),
        component(velocity.y),
        component(velocity.z),
    ])
}

/// `P.  pCell->GetContent()`'s object list at a coordinate (ground layer).
fn cell_has_bridge(sim: &Simulation, coord: ProjectileCoord) -> bool {
    let cell = crate::sim::cell_rect::get_cellclass_fallback(
        sim.resolved_terrain.as_ref(),
        coord.x / 256,
        coord.y / 256,
    );
    cell.bridge_flags_0x1180() & 0x100 != 0
}

/// `PhobosTrajectoryType::GetCellsInProximityRadius` (`:146-195`) and
/// `GetCellsInRectangle` (`:198-420`), ported literally.
fn proximity_cells(
    projectile: &Projectile,
    radius: i32,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> Vec<(u16, u16)> {
    let velocity = projectile.velocity();
    let walk = ProjectileCoord::new(
        velocity.integer_projection().x,
        velocity.integer_projection().y,
        0,
    );
    let magnitude = ((f64::from(walk.x) * f64::from(walk.x)
        + f64::from(walk.y) * f64::from(walk.y))
    .sqrt()) as i32;
    let side_mult = if magnitude != 0 {
        f64::from(radius) / f64::from(magnitude)
    } else {
        0.0
    };
    let cor1 = ProjectileCoord::new(
        (f64::from(walk.y) * side_mult) as i32,
        (f64::from(-walk.x) * side_mult) as i32,
        0,
    );
    let cor4 = ProjectileCoord::new(
        (f64::from(-walk.y) * side_mult) as i32,
        (f64::from(walk.x) * side_mult) as i32,
        0,
    );
    let location = projectile.position();
    let this_cell = cell_of(location);
    let cor1_cell = cell_of(add(location, cor1));
    let cor4_cell = cell_of(add(location, cor4));
    let off1 = (cor1_cell.0 - this_cell.0, cor1_cell.1 - this_cell.1);
    let off4 = (cor4_cell.0 - this_cell.0, cor4_cell.1 - this_cell.1);
    let next_cell = cell_of(add(location, walk));
    let cor2_cell = (next_cell.0 + off1.0, next_cell.1 + off1.1);
    let cor3_cell = (next_cell.0 + off4.0, next_cell.1 + off4.1);
    let mut corners = [cor1_cell, cor2_cell, cor3_cell, cor4_cell];
    let mut corner_index = 0usize;
    for i in 1..4 {
        if corners[corner_index].1 > corners[i].1 {
            corner_index = i;
        }
    }
    let mut cells = rectangle_cells(
        corners[corner_index],
        corners[(corner_index + 1) % 4],
        corners[(corner_index + 2) % 4],
        corners[(corner_index + 3) % 4],
    );
    cells.sort_unstable();
    cells.dedup();
    let _ = (terrain, &mut corners);
    cells
        .into_iter()
        .map(|(x, y)| (x as u16, y as u16))
        .collect()
}

fn cell_of(coord: ProjectileCoord) -> (i32, i32) {
    (coord.x / 256, coord.y / 256)
}

/// `GetCellsInRectangle`'s own order is kept (bottom, left boundary, right
/// boundary, middle, then up one row); the caller dedups cells it visits
/// twice, as the native object-list walk only sees an object once per cell.
fn rectangle_cells(
    bottom: (i32, i32),
    left_mid: (i32, i32),
    right_mid: (i32, i32),
    top: (i32, i32),
) -> Vec<(i32, i32)> {
    let mut cells = vec![bottom];
    if bottom == left_mid || bottom == right_mid {
        // Straight line.
        let mut cur = bottom;
        let dist = (top.0 - bottom.0, top.1 - bottom.1);
        let unit = (dist.0.signum(), dist.1.signum());
        let pace = (dist.0 * unit.0, dist.1 * unit.1);
        let mut cur_n = (pace.1 - pace.0) as f32 / 2.0;
        while cur != top {
            if cur_n > 0.0 {
                cur_n -= pace.0 as f32;
                cur.1 += unit.1;
                cells.push(cur);
            } else if cur_n < 0.0 {
                cur_n += pace.1 as f32;
                cur.0 += unit.0;
                cells.push(cur);
            } else {
                cur_n += (pace.1 - pace.0) as f32;
                cur.0 += unit.0;
                cells.push(cur);
                cur.0 -= unit.0;
                cur.1 += unit.1;
                cells.push(cur);
                cur.0 += unit.0;
                cells.push(cur);
            }
        }
        return cells;
    }
    // Full rectangle: left and right boundaries filled per row.
    let units = |dist: (i32, i32)| (dist.0.signum(), dist.1.signum());
    let paces = |dist: (i32, i32), unit: (i32, i32)| (dist.0 * unit.0, dist.1 * unit.1);
    let left1 = (left_mid.0 - bottom.0, left_mid.1 - bottom.1);
    let left1u = units(left1);
    let left1p = paces(left1, left1u);
    let mut left1n = (left1p.1 - left1p.0) as f32 / 2.0;
    let left2 = (top.0 - left_mid.0, top.1 - left_mid.1);
    let left2u = units(left2);
    let left2p = paces(left2, left2u);
    let mut left2n = (left2p.1 - left2p.0) as f32 / 2.0;
    let right1 = (right_mid.0 - bottom.0, right_mid.1 - bottom.1);
    let right1u = units(right1);
    let right1p = paces(right1, right1u);
    let mut right1n = (right1p.1 - right1p.0) as f32 / 2.0;
    let right2 = (top.0 - right_mid.0, top.1 - right_mid.1);
    let right2u = units(right2);
    let right2p = paces(right2, right2u);
    let mut right2n = (right2p.1 - right2p.0) as f32 / 2.0;
    let mut left = bottom;
    let mut right = bottom;
    let (mut left_next, mut right_next, mut left_skip, mut right_skip) =
        (false, false, false, false);
    let (mut left_continue, mut right_continue) = (false, false);
    while left != top || right != top {
        while left != top {
            if !left_next {
                if left1n > 0.0 {
                    left1n -= left1p.0 as f32;
                    left.1 += left1u.1;
                    if left == left_mid {
                        left_next = true;
                    } else {
                        cells.push(left);
                        break;
                    }
                } else {
                    left1n += left1p.1 as f32;
                    left.0 += left1u.0;
                    if left == left_mid {
                        left_next = true;
                        left_skip = true;
                    }
                }
            } else if left2n >= 0.0 {
                if left_skip {
                    left_skip = false;
                    left2n -= left2p.0 as f32;
                    left.1 += left2u.1;
                } else {
                    left_continue = true;
                    break;
                }
            } else {
                left2n += left2p.1 as f32;
                left.0 += left2u.0;
            }
            if left != right {
                cells.push(left);
            }
        }
        while right != top {
            if !right_next {
                if right1n > 0.0 {
                    right1n -= right1p.0 as f32;
                    right.1 += right1u.1;
                    if right == right_mid {
                        right_next = true;
                    } else {
                        cells.push(right);
                        break;
                    }
                } else {
                    right1n += right1p.1 as f32;
                    right.0 += right1u.0;
                    if right == right_mid {
                        right_next = true;
                        right_skip = true;
                    }
                }
            } else if right2n >= 0.0 {
                if right_skip {
                    right_skip = false;
                    right2n -= right2p.0 as f32;
                    right.1 += right2u.1;
                } else {
                    right_continue = true;
                    break;
                }
            } else {
                right2n += right2p.1 as f32;
                right.0 += right2u.0;
            }
            if right != left {
                cells.push(right);
            }
        }
        let mut middle = left;
        middle.0 += 1;
        while middle.0 < right.0 {
            cells.push(middle);
            middle.0 += 1;
        }
        if left_continue {
            left_continue = false;
            left2n -= left2p.0 as f32;
            left.1 += left2u.1;
            cells.push(left);
        }
        if right_continue {
            right_continue = false;
            right2n -= right2p.0 as f32;
            right.1 += right2u.1;
            cells.push(right);
        }
    }
    cells
}

fn proximity_candidate(
    sim: &Simulation,
    _rules: &RuleSet,
    id: u64,
    source_id: u64,
    owner_house: Option<InternedId>,
    target: ProjectileTarget,
    source_origin: ProjectileCoord,
    position: ProjectileCoord,
    velocity: ProjectileVelocity,
    radius: i32,
    proximity_allies: bool,
    flight: bool,
) -> bool {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return false;
    };
    if !entity.is_object_alive() || entity.lifecycle.in_limbo() || entity.health.current() <= 0 {
        return false;
    }
    let coords =
        crate::sim::movement::ground_pose::object_get_coords(entity, sim.resolved_terrain.as_ref());
    let coords = ProjectileCoord::new(coords.x, coords.y, coords.z);
    if entity.category() == EntityCategory::Structure
        && sim
            .interner
            .object_type(entity.type_ref(), _rules)
            .is_some_and(|kind| kind.invisible_in_game)
    {
        return false;
    }
    let is_target = matches!(target, ProjectileTarget::Entity(t) if t == id);
    if !proximity_allies
        && let Some(owner) = owner_house
        && sim
            .house_alliances
            .is_allied(&sim.interner, owner, entity.owner())
        && !is_target
    {
        return false;
    }
    let velocity_c = ProjectileCoord::new(
        velocity.integer_projection().x,
        velocity.integer_projection().y,
        velocity.integer_projection().z,
    );
    let path = sub(coords, source_origin);
    if dot_i(path, velocity_c) < 0 {
        return false;
    }
    let distance_c = sub(coords, position);
    let next = sub(distance_c, velocity_c);
    if dot_i(next, velocity_c) > 0 {
        return false;
    }
    let cross = cross(distance_c, next);
    let velocity_sq = i64::from(velocity_c.x) * i64::from(velocity_c.x)
        + i64::from(velocity_c.y) * i64::from(velocity_c.y)
        + i64::from(velocity_c.z) * i64::from(velocity_c.z);
    let perpendicular = if velocity_sq > 0 {
        (mag_sq(cross) as f64 / velocity_sq as f64).sqrt()
    } else {
        distance_f64(coords, position)
    };
    if (flight || entity.category() != EntityCategory::Structure)
        && perpendicular > f64::from(radius)
    {
        return false;
    }
    let _ = source_id;
    true
}

fn dot_i(a: ProjectileCoord, b: ProjectileCoord) -> i64 {
    i64::from(a.x) * i64::from(b.x)
        + i64::from(a.y) * i64::from(b.y)
        + i64::from(a.z) * i64::from(b.z)
}

/// `CheckThroughAndSubjectInCell` (`StraightTrajectory.cpp:653-693`).
pub(super) fn through_blocker(
    sim: &Simulation,
    rules: &RuleSet,
    coord: ProjectileCoord,
    source_id: u64,
    target: ProjectileTarget,
    through_vehicles: bool,
    through_building: bool,
) -> Option<u64> {
    let (rx, ry) = ((coord.x / 256) as u16, (coord.y / 256) as u16);
    let occupancy = sim.substrate.occupancy.get(rx, ry)?;
    let owner_house = sim
        .substrate
        .entities
        .get(source_id)
        .map(|entity| entity.owner());
    for occupant in occupancy.iter_layer(crate::sim::movement::locomotor::MovementLayer::Ground) {
        let id = occupant.entity_id;
        let Some(entity) = sim.substrate.entities.get(id) else {
            continue;
        };
        let is_target = matches!(target, ProjectileTarget::Entity(t) if t == id);
        if let Some(owner) = owner_house
            && sim
                .house_alliances
                .is_allied(&sim.interner, owner, entity.owner())
            && !is_target
        {
            continue;
        }
        match entity.category() {
            EntityCategory::Structure => {
                let strange = sim
                    .interner
                    .object_type(entity.type_ref(), rules)
                    .is_some_and(|kind| kind.is_vehicle());
                let invisible = sim
                    .interner
                    .object_type(entity.type_ref(), rules)
                    .is_some_and(|kind| kind.invisible_in_game);
                if invisible {
                    continue;
                }
                let blocked = if strange {
                    !through_vehicles
                } else {
                    !through_building
                };
                if blocked {
                    return Some(id);
                }
            }
            EntityCategory::Unit | EntityCategory::Aircraft => {
                if !through_vehicles {
                    return Some(id);
                }
            }
            _ => {}
        }
    }
    None
}

fn through_allows(
    sim: &Simulation,
    rules: &RuleSet,
    id: u64,
    through_vehicles: bool,
    through_building: bool,
) -> bool {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return false;
    };
    match entity.category() {
        EntityCategory::Structure => {
            let strange = sim
                .interner
                .object_type(entity.type_ref(), rules)
                .is_some_and(|kind| kind.is_vehicle());
            if strange {
                through_vehicles
            } else {
                through_building
            }
        }
        EntityCategory::Unit | EntityCategory::Aircraft => through_vehicles,
        _ => true,
    }
}
