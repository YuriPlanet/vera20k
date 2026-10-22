//! Original BulletClass::DetonateAtCoord Airburst tail469EBA..46A290.

use crate::map::cell_index::NativeCellIdentity;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::ruleset::RuleSet;
use crate::sim::projectile::{ProjectileDetonation, ProjectilePayload, ProjectileTarget};
use crate::sim::world::Simulation;

/// The shared tail calls this after damage/debris, using live Bullet fields.
/// Eight native compass neighbors followed by the retained center identity;
/// Cluster controls neither this count nor a second parent detonation.
pub(super) fn emit(world: &mut Simulation, rules: &RuleSet, detonation: &ProjectileDetonation) {
    // Installed Ares replaces this entire tail with1001A090, including when
    // Splits=false. Its circular AirburstSpread consumer is a separate chain;
    // never silently run the original fixed-nine implementation in that build.
    if crate::rules::ARES_ENABLED {
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
        .and_then(|id| rules.projectile(world.interner.resolve(id)))
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

#[cfg(test)]
#[path = "airburst_tests.rs"]
mod tests;
