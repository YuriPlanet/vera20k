//! `BulletClass::SpawnShrapnel @ 0x0046A310`.

use super::*;

/// The cell whose object lists a Map lookup answered: a real cell, by its own
/// coordinate. The shared dummy cell never holds an object: `MapClass::
/// Place_Down @ 0x005683C0`, the one `CellClass::AddContent` caller, skips a
/// missing cell.
fn cell_list_position(
    cells: &crate::map::resolved_terrain::NativeCellQuery<'_>,
    cell: crate::map::cell_index::NativeCellIdentity,
) -> Option<(u16, u16)> {
    match cell {
        crate::map::cell_index::NativeCellIdentity::Real(_) => {
            let (x, y) = cells.coord(cell);
            Some((x as u16, y as u16))
        }
        crate::map::cell_index::NativeCellIdentity::Dummy => None,
    }
}

fn ground_head(
    world: &Simulation,
    cell: Option<(u16, u16)>,
) -> Option<crate::sim::occupancy::CellObjectMember> {
    let (rx, ry) = cell?;
    world
        .substrate
        .occupancy
        .cell_objects(
            rx,
            ry,
            crate::sim::movement::locomotor::MovementLayer::Ground,
            world.terrain_objects.cell_index().get(&(rx, ry)).copied(),
        )
        .next()
}

/// `BulletClass::SpawnShrapnel @ 0x0046A310` for the bullet at `location`,
/// its Location (`+0x9C`). Every child is constructed, fired and has its
/// effects copied before selecting the next child or running parent damage.
pub(super) fn emit_projectile_shrapnel(
    world: &mut Simulation,
    rules: &RuleSet,
    detonation: &ProjectileDetonation,
    location: ProjectileCoord,
) {
    let Some(parent_projectile) = detonation
        .payload
        .projectile_type()
        .and_then(|kind| rules.projectile(world.interner.resolve(kind)))
    else {
        return;
    };
    let Some(child_weapon_name) = parent_projectile.shrapnel_weapon.as_deref() else {
        return;
    };
    let Some(child_weapon) = rules.weapon(child_weapon_name) else {
        return;
    };
    let Some(child_projectile) = child_weapon
        .projectile
        .as_deref()
        .and_then(|name| rules.projectile(name))
    else {
        return;
    };
    let Some(child_warhead_name) = child_weapon.warhead.as_deref() else {
        return;
    };

    // 46A336..46A3C2: negative count uses the original firer's GetCoords.
    let firer = world.substrate.entities.get(detonation.source_id);
    let distance_cells = firer.map_or(0, |firer| {
        let coord = crate::sim::movement::ground_pose::object_get_coords(
            firer,
            world.resolved_terrain.as_ref(),
        );
        crate::util::native_x87::distance_3d_leptons(
            [coord.x, coord.y, coord.z],
            [location.x, location.y, location.z],
        ) / 256
    });
    let Some(count) = projectile_shrapnel_count(
        parent_projectile.shrapnel_count,
        firer.is_some(),
        distance_cells,
    ) else {
        return;
    };
    let source_owner = firer.map(|source| source.owner());
    let center_rx = crate::util::lepton::lepton_to_cell(location.x);
    let center_ry = crate::util::lepton::lepton_to_cell(location.y);
    // 46A3D6..46A406: GetCell's first ground object must exist and must not
    // be a Building. Terrain can admit the parent and be a later ring target.
    let bullet_cell = match world.resolved_terrain.as_ref() {
        Some(terrain) => {
            let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
            cell_list_position(
                &cells,
                crate::sim::movement::ground_pose::query_object_cell(
                    &cells,
                    crate::sim::components::DriveCoord {
                        x: location.x,
                        y: location.y,
                        z: location.z,
                    },
                ),
            )
        }
        None => u16::try_from(center_rx)
            .ok()
            .zip(u16::try_from(center_ry).ok()),
    };
    let admitted = match ground_head(world, bullet_cell) {
        Some(crate::sim::occupancy::CellObjectMember::Entity(id)) => world
            .substrate
            .entities
            .get(id)
            .is_some_and(|object| object.category() != EntityCategory::Structure),
        Some(crate::sim::occupancy::CellObjectMember::Terrain(_)) => true,
        None => false,
    };
    if !admitted {
        return;
    }

    let construct = |world: &mut Simulation,
                     target: ProjectileTarget,
                     captured_target_coord: Option<ProjectileCoord>,
                     random_words: Option<(i32, i32)>| {
        // The random-cell children (captured) launch through the second branch.
        let random_cell = captured_target_coord.is_some();
        let target_coord = if let Some(captured) = captured_target_coord {
            captured
        } else {
            match target {
                ProjectileTarget::Entity(id) => {
                    let Some(entity) = world.substrate.entities.get(id) else {
                        return;
                    };
                    // `0x0046A614`: the object's GetCoords (vt+0x48), a
                    // building's foundation center (`0x00447AC0`).
                    let coord = crate::sim::movement::ground_pose::object_get_coords(
                        entity,
                        world.resolved_terrain.as_ref(),
                    );
                    ProjectileCoord::new(coord.x, coord.y, coord.z)
                }
                // Terrain+48 is ObjectGetCoords5F65A0: retained Location,
                // including construction Z, never a fresh ground sample.
                ProjectileTarget::Terrain(id) => {
                    let Some(terrain) = world.terrain_objects.objects().get(&id) else {
                        return;
                    };
                    terrain.world_coord()
                }
                ProjectileTarget::Cell { rx, ry } => crate::sim::projectile::cell_target_coord(
                    world.resolved_terrain.as_ref(),
                    rx,
                    ry,
                ),
                ProjectileTarget::None => ProjectileCoord::new(0, 0, 0),
                ProjectileTarget::DummyCell => world
                    .resolved_terrain
                    .as_ref()
                    .map(crate::map::resolved_terrain::ResolvedTerrainGrid::shared_cell_dummy)
                    .as_ref()
                    .map(crate::sim::projectile::dummy_cell_target_coord)
                    .unwrap_or(ProjectileCoord::new(0, 0, 0)),
            }
        };
        let native_unique_id = crate::sim::native_identity::NativeUniqueIdCursor::assign_runtime(
            &mut world.native_unique_ids,
        );
        let spawn = ProjectileSpawn {
            native_unique_id,
            line_trail: crate::sim::projectile::ProjectileLineTrail::from_type(
                child_projectile,
                rules.general.line_trail_color_override,
            ),
            flat: child_projectile.flat,
            source_id: detonation.source_id,
            origin: location,
            target,
            initial_target_position: target_coord,
            payload: ProjectilePayload::new(
                child_weapon.damage,
                world.interner.intern(child_warhead_name),
                Some(world.interner.intern(&child_projectile.id)),
                child_weapon.bright,
            ),
            speed_leptons_per_frame: child_weapon.speed.clamp(1, i32::from(u16::MAX)) as u16,
            velocity: crate::sim::projectile::launch::shrapnel_launch_velocity(
                location,
                target_coord,
                child_weapon.speed,
                random_cell,
            ),
            // Child launch owner: BulletClass::Shrapnel @ 0x0046A310.
            trajectory: ProjectileTrajectory::Ballistic,
            guidance: None,
            visual: ProjectileVisualState::new(
                child_projectile.anim_low as u8,
                child_projectile.anim_high as u8,
                child_projectile.anim_rate as u8,
            ),
            arm_frames: projectile_arm_delay(
                child_projectile.arm,
                target,
                &world.substrate.entities,
            ),
            fuse_frames: None,
            ranged_fuse: child_projectile.rot > 0 || child_projectile.ranged,
            tracks_target: false,
            target_expiry: TargetExpiryPolicy::DetonateAtLastKnown,
            collision: ProjectileCollisionPolicy {
                level: child_projectile.level,
                subject_to_walls: child_projectile.subject_to_walls,
                native_cell_collision: child_projectile.rot <= 0,
                dropping: child_projectile.dropping,
                subject_to_cliffs: child_projectile.subject_to_cliffs,
                flak_scatter: child_projectile.flak_scatter,
                anti_air: child_projectile.aa,
                airburst: child_projectile.airburst,
                inaccurate: child_projectile.inaccurate,
                floater: child_projectile.floater,
                elasticity_bits: child_projectile.elasticity.to_bits(),
                arcing: child_projectile.arcing,
            },
        };
        world.admit_projectile_spawns([spawn], rules);
        // SpawnShrapnel46A887/46AD3E: each child Fire precedes its independent
        // laser and electric-bolt calls. Both use the original firer and the
        // parent's Location, before parent AreaDamage can expire either.
        if child_weapon.is_laser {
            if let Some((rx, ry)) = random_words {
                let _ = crate::sim::cell_rect::get_cellclass_fallback(
                    world.resolved_terrain.as_ref(),
                    rx,
                    ry,
                );
            }
            let _ = laser::create(
                world,
                rules,
                detonation.source_id,
                target,
                0,
                child_weapon,
                Some(location),
            );
        }
        if child_weapon.is_electric_bolt {
            if let Some((rx, ry)) = random_words {
                let _ = crate::sim::cell_rect::get_cellclass_fallback(
                    world.resolved_terrain.as_ref(),
                    rx,
                    ry,
                );
            }
            electric_bolt::create(
                world,
                rules,
                detonation.source_id,
                target,
                child_weapon,
                location,
                None,
            );
        }
    };

    // 46A436..46A509: whole Range rings, head only, excluding the original
    // firer and its allies. Count0 still consumes every eligible ring head:
    // 46A8D9 tests equality after constructing the child.
    let rings = (child_weapon.range_leptons / 256).max(0) as usize;
    let ring_cells = if source_owner.is_some() {
        super::cell_spread::sweep(rings)
    } else {
        &[]
    };
    let mut emitted = 0;
    for &(dx, dy) in ring_cells.iter().skip(1) {
        let rx = (center_rx as i16).wrapping_add(dx);
        let ry = (center_ry as i16).wrapping_add(dy);
        let cell = match world.resolved_terrain.as_ref() {
            Some(terrain) => {
                let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
                cell_list_position(&cells, cells.lookup((rx, ry)))
            }
            None => u16::try_from(rx).ok().zip(u16::try_from(ry).ok()),
        };
        let target = match ground_head(world, cell) {
            Some(crate::sim::occupancy::CellObjectMember::Entity(id)) => {
                if id == detonation.source_id {
                    continue;
                }
                let Some(target) = world.substrate.entities.get(id) else {
                    continue;
                };
                if source_owner.is_some_and(|owner| {
                    crate::map::houses::is_allied_with(
                        world.house_alliances.as_map(),
                        world.interner.resolve(owner),
                        world.interner.resolve(target.owner()),
                    )
                }) {
                    continue;
                }
                ProjectileTarget::Entity(id)
            }
            // 46A4D7 reads the literal head; Terrain's owner virtual4104A0
            // returns null, so House4F9A90 rejects alliance, not the target.
            // No Techno, LegalTarget or health filter belongs in this walk.
            Some(crate::sim::occupancy::CellObjectMember::Terrain(id)) => {
                ProjectileTarget::Terrain(id)
            }
            None => continue,
        };
        construct(world, target, None, None);
        emitted += 1;
        if emitted == count {
            break;
        }
    }
    while emitted < count {
        // 46A942..46ADBE: X then Y; construct/fire/effects precede the next
        // pair of draws. Capture each shared-dummy coordinate at its lookup.
        let (rx, ry) =
            projectile_random_shrapnel_cell(center_rx, center_ry, &mut world.scenario_rng);
        let (target, initial_target_position) = if world.resolved_terrain.is_none() {
            (
                ProjectileTarget::Cell {
                    rx: rx as u16,
                    ry: ry as u16,
                },
                crate::sim::projectile::cell_target_coord(None, rx as u16, ry as u16),
            )
        } else {
            let cell = crate::sim::cell_rect::get_cellclass_fallback(
                world.resolved_terrain.as_ref(),
                rx,
                ry,
            );
            (
                ProjectileTarget::of_cell(&cell),
                crate::sim::projectile::cell_ref_target_coord(&cell),
            )
        };
        construct(world, target, Some(initial_target_position), Some((rx, ry)));
        emitted += 1;
    }
}
