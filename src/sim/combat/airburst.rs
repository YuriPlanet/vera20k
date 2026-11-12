//! Original BulletClass::DetonateAtCoord Airburst tail469EBA..46A290.

use crate::map::cell_index::NativeCellIdentity;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::ruleset::RuleSet;
use crate::sim::projectile::{ProjectileDetonation, ProjectilePayload, ProjectileTarget};
use crate::sim::world::Simulation;

/// The shared tail calls this after damage/debris, using live Bullet fields.
/// Installed Ares replaces the whole tail with `1001A090` (hook `0x469EBA`,
/// [`ares_emit`]), including when `Splits=` is false. Installed Phobos NOPs
/// it and owns both `Airburst=` and `Splits=` (`phobos_airburst`), taking
/// precedence over Ares; Phobos builds never call this.
#[cfg_attr(feature = "phobos", allow(dead_code))]
pub(super) fn emit(world: &mut Simulation, rules: &RuleSet, detonation: &ProjectileDetonation) {
    #[cfg(feature = "ares")]
    ares_emit(world, rules, detonation);
    #[cfg(not(feature = "ares"))]
    stock_emit(world, rules, detonation);
}

/// Eight native compass neighbors followed by the retained center identity;
/// Cluster controls neither this count nor a second parent detonation.
#[cfg(not(feature = "ares"))]
#[cfg_attr(feature = "phobos", allow(dead_code))]
fn stock_emit(world: &mut Simulation, rules: &RuleSet, detonation: &ProjectileDetonation) {
    if crate::rules::PHOBOS_ENABLED {
        return;
    }
    // Native always has an actual `this` Bullet. Receiver-only packets (for
    // example a bare death weapon) do not invent a fan from an impact point.
    let Some(parent) = world.projectiles.get(detonation.projectile_id) else {
        return;
    };
    let location = parent.position();
    let Some(kind) = parent
        .payload()
        .projectile_type()
        .and_then(|id| world.interner.projectile_type(id, rules))
        .filter(|kind| kind.airburst)
    else {
        return;
    };
    let Some(weapon) = kind
        .airburst_weapon
        .as_deref()
        .and_then(|id| rules.weapon(id))
    else {
        return;
    };
    let Some(child) = rules.weapon_projectile(weapon) else {
        return;
    };
    let Some(warhead) = rules.weapon_warhead(weapon) else {
        return;
    };
    let Some(terrain) = world.resolved_terrain.as_ref() else {
        return;
    };
    let center = NativeCellQuery::canonical(terrain).lookup_world(location.x, location.y);
    for direction in 0..9 {
        let target = {
            let cells = NativeCellQuery::canonical(world.resolved_terrain.as_ref().unwrap());
            let cell = cells.neighbor(center, direction);
            match cell {
                NativeCellIdentity::Real(_) => {
                    let (x, y) = cells.coord(cell);
                    ProjectileTarget::Cell {
                        rx: x as u16,
                        ry: y as u16,
                    }
                }
                NativeCellIdentity::Dummy => ProjectileTarget::DummyCell,
            }
        };
        // Source can have expired in the parent's synchronous AreaDamage.
        // current_detonation follows the existing pointer-expiry authority;
        // do not retain a firing-side house or attacker snapshot here.
        let live = world.projectiles.current_detonation(*detonation);
        let native_unique_id = world.next_native_runtime_id();
        let bullet_id = world.substrate.allocate_stable_id();
        let payload = ProjectilePayload::new(
            weapon.damage,
            world.interner.intern(&warhead.id),
            Some(world.interner.intern(&child.id)),
            false,
        );
        // Native constructs before drawing. Reserve its ID in that order,
        // then let the existing admission construct/publish after the draw.
        // No world callback occurs between these points: RNG reads no object
        // state, and publication completes before the next child/Logic visit.
        let velocity = crate::sim::projectile::launch::airburst_launch_velocity(
            weapon.speed,
            &mut world.scenario_rng,
        );
        if let Some(bullet) = super::FiredBullet::launched(
            world,
            rules,
            native_unique_id,
            live.source_id,
            target,
            payload,
            50,
            location,
            velocity,
        ) {
            super::admit_fired_bullet(world, rules, weapon, bullet_id, bullet);
        }
    }
}

/// Ares 3.0 BulletClass_DetonateAt_Splits (`1001A090`, hook `0x469EBA`, to
/// `0x46A290`). Nothing happens without split behaviour (`Airburst=` or
/// Ares `Splits=`, [`ProjectileType::airburst_arm`]) or an `AirburstWeapon=`.
/// The destination is the bullet's coordinate, or with `AroundTarget=`
/// (unset: `Splits=`) its Target's when it has one.
/// - Without `Splits=` every map cell within `AirburstSpread=` of the
///   destination cell (CellRangeIterator, row-major) takes one child.
/// - With `Splits=` the targets are, in TechnoClass::Array order, every
///   Techno in the playfield, on the map and alive, not the firer unless
///   `RetargetSelf=`, whose house the warhead's CanAffectHouse (`10051B70`)
///   admits for the firer's house, closer than 1280 leptons to the
///   destination and, unless the child projectile is `AA=`, not in the air;
///   then random cells within 3 of the destination cell (two Scenario
///   `RandomRanged(-3, 3)`, x first; a cell off the map is the shared dummy)
///   until there are `Cluster=` of them. Each of the `Cluster=` children
///   keeps the bullet's Target unless it has none or `RetargetAccuracy=` is
///   below a Scenario RandomDouble; then it takes a random target
///   (`RandomRanged(0, count - 1)`, again for the firer when a second
///   RandomDouble exceeds 0.5), whose slot the last target fills.
///
/// Each child is `BulletTypeExt::CreateBullet` (`AirburstWeapon=`'s
/// damage, warhead, speed, `Bright=` and `ProjectileRange=`) moved from the
/// bullet's location ([`ares_airburst_launch_velocity`]).
///
/// [`ProjectileType::airburst_arm`]: crate::rules::projectile_type::ProjectileType::airburst_arm
/// [`ares_airburst_launch_velocity`]: crate::sim::projectile::launch::ares_airburst_launch_velocity
#[cfg(feature = "ares")]
#[cfg_attr(feature = "phobos", allow(dead_code))]
fn ares_emit(world: &mut Simulation, rules: &RuleSet, detonation: &ProjectileDetonation) {
    let Some(parent) = world.projectiles.get(detonation.projectile_id) else {
        return;
    };
    let location = parent.position();
    let Some(kind) = parent
        .payload()
        .projectile_type()
        .and_then(|id| world.interner.projectile_type(id, rules))
        .filter(|kind| kind.airburst_arm())
    else {
        return;
    };
    let Some(weapon) = kind
        .airburst_weapon
        .as_deref()
        .and_then(|id| rules.weapon(id))
    else {
        return;
    };
    let Some(child) = weapon
        .projectile
        .as_deref()
        .and_then(|id| rules.projectile(id))
    else {
        return;
    };
    let Some(warhead) = weapon.warhead.as_deref().and_then(|id| rules.warhead(id)) else {
        return;
    };
    let ext = &kind.bullet_type_ext;
    let splits = ext.splits;
    // The source can have expired in the parent's synchronous AreaDamage.
    let live = world.projectiles.current_detonation(*detonation);
    let firer = live.source_id;
    let firer_house = world
        .substrate
        .entities
        .get(firer)
        .map(|entity| entity.owner());
    let destination = if ext.around_target.unwrap_or(splits) {
        target_get_coords(world, live.target).unwrap_or(location)
    } else {
        location
    };
    let center = (destination.x / 256, destination.y / 256);

    let mut targets = Vec::new();
    let cluster = if !splits {
        targets = cell_range(world, center, f64::from_bits(ext.airburst_spread.bits()));
        targets.len()
    } else {
        let defaults = crate::rules::ares_warhead::AresWarheadRules::default();
        let affects = warhead.ares_rules().unwrap_or(&defaults);
        let terrain = world.resolved_terrain.as_ref();
        for candidate in world.substrate.entities.values_sorted() {
            if !candidate.in_playfield()
                || !candidate.lifecycle.cell_marked()
                || candidate.health.current() <= 0
                || (!ext.retarget_self && candidate.stable_id() == firer)
            {
                continue;
            }
            let allied = firer_house.is_some_and(|house| {
                crate::sim::combat::combat_weapon::is_ally_by_object(
                    Some(&world.house_alliances),
                    &world.interner,
                    house,
                    candidate.owner(),
                )
            });
            if !affects.can_affect_house(
                firer_house.is_some(),
                firer_house == Some(candidate.owner()),
                allied,
                warhead.affects_allies,
            ) {
                continue;
            }
            let at = crate::sim::movement::ground_pose::object_get_coords(candidate, terrain);
            let (dx, dy, dz) = (
                i64::from(destination.x) - i64::from(at.x),
                i64::from(destination.y) - i64::from(at.y),
                i64::from(destination.z) - i64::from(at.z),
            );
            // `DistanceFrom < 1280.0`, on the exact integer square.
            if dx * dx + dy * dy + dz * dz >= 1280 * 1280 {
                continue;
            }
            if child.aa
                || !crate::sim::movement::air_movement::is_high_flying(
                    candidate,
                    terrain,
                    Some((rules, &world.interner)),
                )
            {
                targets.push(ProjectileTarget::Entity(candidate.stable_id()));
            }
        }
        let cluster = usize::try_from(kind.cluster).unwrap_or(0);
        while targets.len() < cluster {
            let x = world.scenario_rng.next_range_i32_inclusive(-3, 3);
            let y = world.scenario_rng.next_range_i32_inclusive(-3, 3);
            let cell = (
                (center.0 as i16).wrapping_add(x as i16),
                (center.1 as i16).wrapping_add(y as i16),
            );
            targets.push(map_cell_target(world, cell).unwrap_or(ProjectileTarget::DummyCell));
        }
        cluster
    };

    for index in 0..cluster {
        let target = if !splits {
            targets[index]
        } else {
            let accuracy = f64::from_bits(ext.retarget_accuracy.bits());
            if live.target != ProjectileTarget::None
                && accuracy >= world.scenario_rng.random_double()
            {
                live.target
            } else {
                let last = targets.len() as i32 - 1;
                let mut pick = world.scenario_rng.next_range_i32_inclusive(0, last) as usize;
                if targets[pick] == ProjectileTarget::Entity(firer)
                    && world.scenario_rng.random_double() > 0.5
                {
                    pick = world.scenario_rng.next_range_i32_inclusive(0, last) as usize;
                }
                targets.swap_remove(pick)
            }
        };
        if target != ProjectileTarget::None {
            spawn_ares_child(
                world, rules, weapon, child, warhead, firer, target, location,
            );
        }
    }
}

/// `BulletTypeExt::CreateBullet` then `BulletClass::MoveTo` for one child.
#[cfg(feature = "ares")]
#[cfg_attr(feature = "phobos", allow(dead_code))]
#[allow(clippy::too_many_arguments)]
fn spawn_ares_child(
    world: &mut Simulation,
    rules: &RuleSet,
    weapon: &crate::rules::weapon_type::WeaponType,
    child: &crate::rules::projectile_type::ProjectileType,
    warhead: &crate::rules::warhead_type::WarheadType,
    firer: u64,
    target: ProjectileTarget,
    location: crate::sim::projectile::ProjectileCoord,
) {
    let native_unique_id = world.next_native_runtime_id();
    let bullet_id = world.substrate.allocate_stable_id();
    let payload = ProjectilePayload::new(
        weapon.damage,
        world.interner.intern(&warhead.id),
        Some(world.interner.intern(&child.id)),
        weapon.bright,
    )
    // CreateBullet sets Range to the weapon's ProjectileRange, which only a
    // `Ranged=` type spends (as Fire_CreateBullet 1001ADE0 does).
    .with_ares_range(child.ranged.then_some(weapon.ares_projectile_range));
    // 1001A6B0: one Scenario draw after the constructor.
    let velocity = crate::sim::projectile::launch::ares_airburst_launch_velocity(
        weapon.speed,
        &mut world.scenario_rng,
    );
    if let Some(bullet) = super::FiredBullet::launched(
        world,
        rules,
        native_unique_id,
        firer,
        target,
        payload,
        weapon.speed,
        location,
        velocity,
    ) {
        super::admit_fired_bullet(world, rules, weapon, bullet_id, bullet);
    }
}

/// The Target's GetCoords (vt+0x48); `None` without a Target.
#[cfg(feature = "ares")]
#[cfg_attr(feature = "phobos", allow(dead_code))]
fn target_get_coords(
    world: &Simulation,
    target: ProjectileTarget,
) -> Option<crate::sim::projectile::ProjectileCoord> {
    let terrain = world.resolved_terrain.as_ref();
    match target {
        ProjectileTarget::Entity(id) => {
            let at = crate::sim::movement::ground_pose::object_get_coords(
                world.substrate.entities.get(id)?,
                terrain,
            );
            Some(crate::sim::projectile::ProjectileCoord::new(
                at.x, at.y, at.z,
            ))
        }
        ProjectileTarget::Cell { rx, ry } => {
            Some(crate::sim::projectile::cell_ground_coord(terrain, rx, ry))
        }
        ProjectileTarget::Terrain(id) => {
            Some(world.terrain_objects.objects().get(&id)?.world_coord())
        }
        ProjectileTarget::DummyCell | ProjectileTarget::None => None,
    }
}

/// `CellRangeIterator<CellClass>` (YRpp `Helpers/Iterators.h`): the square
/// of `floor(radius + 0.99) * 2 + 1` cells from `center`, row-major, keeping
/// the cells within `radius` that `TryGetCellAt` finds. The square is clipped
/// to the map first, which yields the same cells.
#[cfg(feature = "ares")]
#[cfg_attr(feature = "phobos", allow(dead_code))]
fn cell_range(world: &Simulation, center: (i32, i32), radius: f64) -> Vec<ProjectileTarget> {
    let half = (radius + 0.99).floor();
    if !half.is_finite() || half < 0.0 {
        return Vec::new();
    }
    let half = half.min(f64::from(0x200)) as i32;
    let range = half * 2 + 1;
    let (width, height) = world
        .resolved_terrain
        .as_ref()
        .map_or((0x200, 0x200), |terrain| {
            (i32::from(terrain.width()), i32::from(terrain.height()))
        });
    let left = (center.0 - half).max(0);
    let top = (center.1 - half).max(0);
    let right = (center.0 - half + range).min(width - 1);
    let bottom = (center.1 - half + range).min(height - 1);
    let radius_squared = radius * radius;
    let mut cells = Vec::new();
    for y in top..=bottom {
        for x in left..=right {
            let (dx, dy) = (f64::from(x - center.0), f64::from(y - center.1));
            if dx * dx + dy * dy > radius_squared {
                continue;
            }
            if let Some(target) = map_cell_target(world, (x as i16, y as i16)) {
                cells.push(target);
            }
        }
    }
    cells
}

/// The map's real cell at `cell` as a target, or `None` off the map.
#[cfg(feature = "ares")]
#[cfg_attr(feature = "phobos", allow(dead_code))]
fn map_cell_target(world: &Simulation, cell: (i16, i16)) -> Option<ProjectileTarget> {
    let Some(terrain) = world.resolved_terrain.as_ref() else {
        return Some(ProjectileTarget::Cell {
            rx: cell.0 as u16,
            ry: cell.1 as u16,
        });
    };
    match NativeCellQuery::canonical(terrain).lookup(cell) {
        NativeCellIdentity::Real(_) => Some(ProjectileTarget::Cell {
            rx: cell.0 as u16,
            ry: cell.1 as u16,
        }),
        NativeCellIdentity::Dummy => None,
    }
}

#[cfg(test)]
#[path = "airburst_tests.rs"]
mod tests;
