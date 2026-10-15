//! LaserDraw construction facts. The ordered lifecycle stream hands copied
//! coordinates to presentation; lasers have no simulation object or pointer
//! registration. Original: SpawnLaser6FD210, support44ABD0, ctor54FE60.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::rules::weapon_type::WeaponType;
use crate::sim::intern::InternedId;
use crate::sim::projectile::{ProjectileCoord, ProjectileTarget};
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
    let weapon = if source.category() == EntityCategory::Structure {
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
    let prism = source.category() == EntityCategory::Structure && rules.is_prism_type(obj);
    let supported = prism && prism_support_count > 0;
    // RESIDUAL: other non-building house-color callers use width1 vs native2
    // (6FF571); their stock reachability is unestablished, a cosmetic 1px gap.
    let width = if prism {
        if supported { 5 } else { 3 }
    } else if source.category() == EntityCategory::Unit && weapon.is_house_color {
        2
    } else {
        1
    };
    if let Some(laser) = create(
        world,
        rules,
        source_id,
        target.into(),
        weapon_index,
        weapon,
        None,
    ) {
        // FireAt6FF50A..6FF571 changes the returned LaserDraw after the
        // shared SpawnLaser constructor. Direct shrapnel calls omit this.
        laser.width = width;
        laser.supported = supported;
    }
}

/// TechnoClass::SpawnLaser6FD210. The caller owns the IsLaser admission
/// policy. Explicit source coordinates (SpawnShrapnel46A8AC/46AD81) keep
/// ZAdjust0; only the NullCoord arm computes GetFLH and building ZAdjust.
/// StaticInit6F2A50 establishes NullCoordB0EA90 as [0,0,0], so an explicitly
/// supplied zero coordinate takes the same arm as `None`.
pub(crate) fn create<'w>(
    world: &'w mut Simulation,
    rules: &RuleSet,
    source_id: u64,
    target: ProjectileTarget,
    weapon_index: i32,
    weapon: &WeaponType,
    from: Option<ProjectileCoord>,
) -> Option<&'w mut LaserBirth> {
    let source = world.substrate.entities.get(source_id)?;
    let color = if weapon.is_house_color {
        LaserColor::House(source.owner())
    } else {
        LaserColor::Explicit {
            inner: weapon.laser_inner_color,
            outer: weapon.laser_outer_color,
            spread: weapon.laser_outer_spread,
        }
    };
    let (from, z_adjust) =
        if let Some(from) = from.filter(|coord| *coord != ProjectileCoord::new(0, 0, 0)) {
            (from, 0)
        } else {
            let object = rules.object(world.interner.resolve(source.type_ref()))?;
            let from = super::fire_coord::fire_coordinate(
                world,
                rules,
                &super::fire_coord::FireSource::of_entity(source),
                object,
                weapon_index,
                (source.weapon_burst.index() & 1) as u8,
                Default::default(),
            )
            .coord;
            (
                from,
                super::fire_coord::effect_z_adjust(world, source, from),
            )
        };
    let to = super::fire_coord::effect_target_coordinate(world, rules, target)?;
    Some(construct(
        world,
        from,
        to,
        z_adjust,
        weapon.laser_duration,
        color,
    ))
}

/// LaserDraw54FE60 initializes every caller's width1/unsupported object;
/// caller-specific postprocessing mutates that one returned birth.
fn construct(
    world: &mut Simulation,
    from: ProjectileCoord,
    to: ProjectileCoord,
    z_adjust: i32,
    duration: i32,
    color: LaserColor,
) -> &mut LaserBirth {
    world
        .lifecycle_outputs
        .push(LifecycleOutput::LaserCreated(LaserBirth {
            frame: world.session.binary_frame as i32,
            from,
            to,
            z_adjust,
            duration,
            width: 1,
            supported: false,
            color,
        }));
    let Some(LifecycleOutput::LaserCreated(laser)) = world.lifecycle_outputs.last_mut() else {
        unreachable!("newly appended laser birth")
    };
    laser
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
    let color = LaserColor::House(source.owner());
    construct(
        world,
        from,
        to,
        0,
        rules.general.prism_support.duration,
        color,
    )
    .width = 3;
}
