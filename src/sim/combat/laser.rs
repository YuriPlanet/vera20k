//! LaserDraw construction facts. The ordered lifecycle stream hands copied
//! coordinates to presentation; lasers have no simulation object or pointer
//! registration. Original: SpawnLaser6FD210, support44ABD0, ctor54FE60.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::rules::weapon_type::WeaponType;
use crate::sim::intern::InternedId;
use crate::sim::projectile::ProjectileCoord;
use crate::sim::world::{LifecycleOutput, Simulation};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LaserBirth {
    pub frame: i32,
    pub from: ProjectileCoord,
    pub to: ProjectileCoord,
    pub z_adjust: i32,
    pub duration: i32,
    pub width: i32,
    /// Laser+21: the supported Prism main beam's doubled first color.
    pub supported: bool,
    pub color: LaserColor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LaserColor {
    /// House+56FC is resolved once at construction by the existing client
    /// color owner. Capture the owner now, never look up the firer later.
    House(InternedId),
    Explicit {
        inner: [u8; 3],
        outer: [u8; 3],
        spread: [u8; 3],
    },
}

/// Techno6FD210 recomputes GetFLH after FireAt advances its burst/occupant.
/// The caller supplies the support count sampled before ProcessDelayedFire
/// consumes it; it is not a second stored count.
pub(crate) fn fired(
    world: &mut Simulation,
    rules: &RuleSet,
    source_id: u64,
    target: super::TargetKind,
    weapon_index: i32,
    weapon: &WeaponType,
    prism_support_count: i32,
) {
    if !weapon.is_laser {
        return;
    }
    let Some(source) = world.substrate.entities.get(source_id) else {
        return;
    };
    let Some(obj) = rules.object(world.interner.resolve(source.type_ref())) else {
        return;
    };
    // FireAt6FF4CC gates on the fired weapon above, but its building arm
    // 6FF4EA re-reads GetCurrentWeapon after occupant/burst advancement.
    // Only the payload switches: GetFLH still receives the selected index.
    let weapon = if source.category == EntityCategory::Structure {
        let Some(current) = super::combat_weapon::current_weapon(
            source,
            obj,
            &world.substrate.entities,
            rules,
            &world.interner,
        ) else {
            return;
        };
        current
    } else {
        weapon
    };
    let from = super::fire_coord::fire_coordinate(
        world,
        rules,
        &super::fire_coord::FireSource::of_entity(source),
        obj,
        weapon_index,
        (source.weapon_burst.index() & 1) as u8,
        Default::default(),
    )
    .coord;
    let to = match target {
        super::TargetKind::Entity(id) => {
            let Some(target) = world.substrate.entities.get(id) else {
                return;
            };
            // Object+58 / Techno+A4 share this coordinate for the selected
            // GAPOWR (native constructor/read TargetCoordOffset=0). The
            // existing nonzero shipyard TargetCoordOffset residual remains.
            let p = crate::sim::movement::ground_pose::object_get_coords(
                target,
                world.resolved_terrain.as_ref(),
            );
            ProjectileCoord::new(p.x, p.y, p.z)
        }
        super::TargetKind::Cell(x, y) => {
            crate::sim::projectile::cell_target_coord(world.resolved_terrain.as_ref(), x, y)
        }
    };
    let mut z_adjust = 0;
    if source.category == EntityCategory::Structure {
        let location = crate::sim::movement::ground_pose::object_location(
            source,
            world.resolved_terrain.as_ref(),
        );
        let base =
            crate::sim::movement::ground_pose::building_render_order_parts(location, false, false)
                .0;
        let project_y = |x, y, z| crate::util::lepton::absolute_leptons_to_screen(x, y, z).1 as i32;
        z_adjust = project_y(from.x, from.y, from.z)
            .wrapping_sub(project_y(base.x, base.y, base.z))
            .min(0);
    }
    let prism = source.category == EntityCategory::Structure && rules.is_prism_type(obj);
    let supported = prism && prism_support_count > 0;
    // RESIDUAL: other non-building house-color callers use width1 vs native2
    // (6FF571); their stock reachability is unestablished, a cosmetic 1px gap.
    let width = if prism {
        if supported { 5 } else { 3 }
    } else if source.category == EntityCategory::Unit && weapon.is_house_color {
        2
    } else {
        1
    };
    let color = if weapon.is_house_color {
        LaserColor::House(source.owner())
    } else {
        LaserColor::Explicit {
            inner: weapon.laser_inner_color,
            outer: weapon.laser_outer_color,
            spread: weapon.laser_outer_spread,
        }
    };
    world
        .lifecycle_outputs
        .push(LifecycleOutput::LaserCreated(LaserBirth {
            frame: world.session.binary_frame as i32,
            from,
            to,
            z_adjust,
            duration: weapon.laser_duration,
            width,
            supported,
            color,
        }));
}

/// Building44ABD0's delayed support emission is unconditional once due.
/// It uses weapon0's current FLH and the copied endpoint stored when armed.
pub(crate) fn support(
    world: &mut Simulation,
    rules: &RuleSet,
    source_id: u64,
    to: ProjectileCoord,
) {
    let Some(source) = world.substrate.entities.get(source_id) else {
        return;
    };
    let Some(obj) = rules.object(world.interner.resolve(source.type_ref())) else {
        return;
    };
    let from = super::fire_coord::fire_coordinate(
        world,
        rules,
        &super::fire_coord::FireSource::of_entity(source),
        obj,
        0,
        (source.weapon_burst.index() & 1) as u8,
        Default::default(),
    )
    .coord;
    world
        .lifecycle_outputs
        .push(LifecycleOutput::LaserCreated(LaserBirth {
            frame: world.session.binary_frame as i32,
            from,
            to,
            z_adjust: 0,
            duration: rules.general.prism_support.duration,
            width: 3,
            supported: false,
            color: LaserColor::House(source.owner()),
        }));
}
