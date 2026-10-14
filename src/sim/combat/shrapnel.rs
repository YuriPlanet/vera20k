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

/// `BulletClass::SpawnShrapnel @ 0x0046A310` for the bullet at `location`,
/// its Location (`+0x9C`), which the count, the admission, the ring centre and
/// every fragment's origin read; the impact coordinate never reaches it.
pub(super) fn emit_projectile_shrapnel(
    detonation: &ProjectileDetonation,
    location: ProjectileCoord,
    entities: &EntityStore,
    occupancy: &OccupancyGrid,
    terrain_object_cells: &BTreeMap<(u16, u16), u64>,
    rules: &RuleSet,
    interner: &mut StringInterner,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    house_alliances: &HouseAllianceMap,
    scenario_rng: &mut SimRng,
    native_unique_ids: &mut Option<crate::sim::native_identity::NativeUniqueIdCursor>,
    out: &mut CombatEmit,
) {
    let Some(parent_weapon) = rules.weapon(interner.resolve(detonation.payload.weapon)) else {
        return;
    };
    let Some(parent_projectile) = parent_weapon
        .projectile
        .as_deref()
        .and_then(|name| rules.projectile(name))
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
        log::debug!(
            "Projectile {} shrapnel skipped: child projectile constructor unavailable",
            detonation.projectile_id
        );
        return;
    };
    let Some(child_warhead_name) = child_weapon.warhead.as_deref() else {
        return;
    };

    // `0x0046A336..0x0046A3C2`: a negative ShrapnelCount is lessened by the
    // whole cells from the bullet's Location to its firer's GetCoords
    // (`[bullet+0xB0]`, vt+0x48 at `0x0046A370`), through `0x0041C380` and a
    // signed /256; a bullet without a firer gets 3.
    let firer = entities.get(detonation.source_id);
    let distance_cells = firer.map_or(0, |firer| {
        let coord = crate::sim::movement::ground_pose::object_get_coords(firer, terrain);
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

    // The bullet's cell (`vt+0x1B8`, `0x0041BEA0`) centres the rings.
    let center_rx = crate::util::lepton::lepton_to_cell(location.x);
    let center_ry = crate::util::lepton::lepton_to_cell(location.y);
    // `0x0046A3D6..0x0046A406`: shrapnel is thrown only when the bullet's cell
    // (GetCell, vt+0x1BC, `0x005F6960`) holds a first ground object (`+0xE4`)
    // that is not a building (What_Am_I 6). The second GetCell (`0x0046A3F2`)
    // answers the same cell. A missed lookup's dummy cell holds none.
    let cells = terrain.map(crate::map::resolved_terrain::NativeCellQuery::canonical);
    let bullet_cell = match cells.as_ref() {
        Some(cells) => cell_list_position(
            cells,
            crate::sim::movement::ground_pose::query_object_cell(
                cells,
                crate::sim::components::DriveCoord {
                    x: location.x,
                    y: location.y,
                    z: location.z,
                },
            ),
        ),
        // Mapless fixtures read the coordinate's own cell.
        None => u16::try_from(center_rx)
            .ok()
            .zip(u16::try_from(center_ry).ok()),
    };
    let ground_head = |cell: Option<(u16, u16)>| {
        let (rx, ry) = cell?;
        occupancy
            .cell_objects(
                rx,
                ry,
                crate::sim::movement::locomotor::MovementLayer::Ground,
                terrain_object_cells.get(&(rx, ry)).copied(),
            )
            .next()
    };
    let admitted = match ground_head(bullet_cell) {
        Some(crate::sim::occupancy::CellObjectMember::Entity(id)) => entities
            .get(id)
            .is_some_and(|object| object.category() != EntityCategory::Structure),
        Some(crate::sim::occupancy::CellObjectMember::Terrain(_)) => true,
        None => false,
    };
    if !admitted {
        return;
    }
    // Random CellClass selections must capture their target coordinate at the
    // lookup call point: every miss returns the same mutable process dummy, so
    // resolving a collected list afterward would give all missed children the
    // final lookup's coordinate. Entity entries deliberately remain live reads
    // until child construction, preserving their existing behavior.
    let mut targets: Vec<(ProjectileTarget, Option<ProjectileCoord>)> =
        Vec::with_capacity(count as usize);
    // `0x0046A436..0x0046A48B`: the child weapon's Range (`+0xB4`) divided by
    // 256 toward zero names the last ring; rings 1 through it are walked,
    // never the bullet's cell. `0x0046A4E5..0x0046A4F5`: a ring cell's first
    // object is a target only while the bullet has a firer, so with the firer
    // gone every fragment takes a random cell.
    let rings = (child_weapon.range_leptons / 256).max(0) as usize;
    let ring_cells = if firer.is_some() {
        super::cell_spread::sweep(rings)
    } else {
        &[]
    };
    let source_owner = firer.map(|source| source.owner());
    for &(dx, dy) in ring_cells.iter().skip(1) {
        // `0x0046A496..0x0046A4D2`: the ring cell, its packed words added to
        // the centre's, through `MapClass::operator[] @ 0x005657A0`.
        let rx = (center_rx as i16).wrapping_add(dx);
        let ry = (center_ry as i16).wrapping_add(dy);
        let cell = match cells.as_ref() {
            Some(cells) => cell_list_position(cells, cells.lookup((rx, ry))),
            None => u16::try_from(rx).ok().zip(u16::try_from(ry).ok()),
        };
        // `0x0046A4D7..0x0046A509`: the head of its ground list, unless it is
        // the firer or the firer's house is allied to it
        // (`Is_Ally_ByObject @ 0x004F9A90`).
        let target_id = match ground_head(cell) {
            Some(crate::sim::occupancy::CellObjectMember::Entity(id)) => id,
            // A tree heading the list is a native target (a houseless object
            // is no ally), but a VERA bullet cannot target a TerrainClass:
            // the cell yields no fragment, and nothing behind the tree is read.
            Some(crate::sim::occupancy::CellObjectMember::Terrain(_)) | None => continue,
        };
        if target_id == detonation.source_id {
            continue;
        }
        let Some(target) = entities.get(target_id) else {
            continue;
        };
        let allied = source_owner.is_some_and(|owner| {
            crate::map::houses::is_allied_with(
                house_alliances,
                interner.resolve(owner),
                interner.resolve(target.owner()),
            )
        });
        if allied {
            continue;
        }
        targets.push((ProjectileTarget::Entity(target_id), None));
        // `0x0046A8D1..0x0046A8E0`: the walk ends once the count is reached,
        // tested after each fragment, so a zero count walks every ring and
        // takes no random cell (`0x0046A92C..0x0046A936`).
        if targets.len() == count as usize {
            break;
        }
    }
    while targets.len() < count as usize {
        let (rx, ry) = projectile_random_shrapnel_cell(center_rx, center_ry, scenario_rng);
        let selected = crate::sim::cell_rect::get_cellclass_fallback(terrain, rx, ry);
        let (target, initial_target_position) = if terrain.is_none() {
            // Rules-less/terrain-less fixtures historically retain a stable
            // Cell target and flat fallback surface. Native always has a map;
            // do not invent a persistent process-dummy pointer for this Rust
            // compatibility path without separate evidence.
            let target = ProjectileTarget::Cell {
                rx: rx as u16,
                ry: ry as u16,
            };
            (
                target,
                crate::sim::projectile::cell_target_coord(terrain, rx as u16, ry as u16),
            )
        } else {
            match selected {
                crate::sim::cell_rect::CellRef::Real(cell) => {
                    let target = ProjectileTarget::Cell {
                        rx: cell.rx,
                        ry: cell.ry,
                    };
                    (
                        target,
                        crate::sim::projectile::cell_target_coord(terrain, cell.rx, cell.ry),
                    )
                }
                crate::sim::cell_rect::CellRef::Dummy { cell } => (
                    ProjectileTarget::DummyCell,
                    crate::sim::projectile::dummy_cell_target_coord(&cell),
                ),
            }
        };
        targets.push((target, Some(initial_target_position)));
    }

    for (target, captured_target_coord) in targets {
        // The random-cell children (captured) launch through the second branch.
        let random_cell = captured_target_coord.is_some();
        let target_coord = if let Some(captured) = captured_target_coord {
            captured
        } else {
            match target {
                ProjectileTarget::Entity(id) => {
                    let Some(entity) = entities.get(id) else {
                        continue;
                    };
                    // `0x0046A614`: the object's GetCoords (vt+0x48), a
                    // building's foundation center (`0x00447AC0`).
                    let coord =
                        crate::sim::movement::ground_pose::object_get_coords(entity, terrain);
                    ProjectileCoord::new(coord.x, coord.y, coord.z)
                }
                ProjectileTarget::Cell { rx, ry } => {
                    crate::sim::projectile::cell_target_coord(terrain, rx, ry)
                }
                ProjectileTarget::None => ProjectileCoord::new(0, 0, 0),
                ProjectileTarget::DummyCell => terrain
                    .map(crate::map::resolved_terrain::ResolvedTerrainGrid::shared_cell_dummy)
                    .as_ref()
                    .map(crate::sim::projectile::dummy_cell_target_coord)
                    .unwrap_or(ProjectileCoord::new(0, 0, 0)),
            }
        };
        let native_unique_id =
            crate::sim::native_identity::NativeUniqueIdCursor::assign_runtime(native_unique_ids);
        out.projectile_spawns.push(ProjectileSpawn {
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
                interner.intern(child_warhead_name),
                interner.intern(child_weapon_name),
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
            arm_frames: projectile_arm_delay(child_projectile.arm, target, entities),
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
        });
    }
}
