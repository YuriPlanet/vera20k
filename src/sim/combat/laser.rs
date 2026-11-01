//! LaserDraw construction facts. The ordered lifecycle stream hands copied
//! coordinates to presentation; lasers have no simulation object or pointer
//! registration. Original: SpawnLaser6FD210, support44ABD0, ctor54FE60.

use crate::map::entities::EntityCategory;
use crate::rules::flh::Flh;
use crate::rules::phobos_laser_tracking::PositionFollow;
use crate::rules::ruleset::RuleSet;
use crate::rules::weapon_type::WeaponType;
use crate::sim::intern::InternedId;
use crate::sim::projectile::{ProjectileCoord, ProjectileTarget};
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::world::{FrameChannel, LifecycleOutput, Simulation};
use crate::util::native_x87::NativeF32Bits;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaserBirth {
    pub frame: i32,
    pub from: ProjectileCoord,
    pub to: ProjectileCoord,
    pub z_adjust: i32,
    pub duration: i32,
    pub width: i32,
    /// Laser+21: the supported Prism main beam's doubled first color.
    pub supported: bool,
    pub color: LaserColor,
    /// Laser+58, copied by54FE60. Ordinary lasers fade1→0; the delayed
    /// ring caller4A778E/4A785F retains0.5 as its other endpoint.
    pub fade_end: NativeF32Bits,
    /// Phobos `LaserRT` data: which live objects `from`/`to` follow. Not a
    /// LaserDraw field; presentation refreshes the copy each frame.
    pub track: Option<LaserTrack>,
}

/// Phobos `LaserRT::TrackingData`: the retained objects and the frozen
/// muzzle facts needed to move one LaserDraw's endpoints. A laser is not a
/// simulation object, so this rides on the copied birth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaserTrack {
    /// The followed firer; cleared once it dies or converts under
    /// `StopOnFirerConvert`.
    firer: Option<u64>,
    /// The followed target. Phobos casts it to `ObjectClass*`, so cells are
    /// never tracked.
    target: Option<ProjectileTarget>,
    weapon_index: i32,
    /// `CurrentBurstIndex` frozen at creation; `GetFLH` mirrors the lateral
    /// FLH by its parity.
    burst_parity: u8,
    /// `GetBurstFLH`'s fallback: the weapon slot's ART FLH read at creation.
    /// `GetFLH6F3B28` adds this base to the stored weapon FLH again.
    base_flh: Flh,
    /// `initial source - tracked world FLH` at creation.
    saved_offset: ProjectileCoord,
    stop_on_firer_convert: bool,
    original_type: Option<InternedId>,
}

impl LaserBirth {
    /// `LaserDrawClass::Update @ 0x00550173`'s LaserRT arm (Phobos
    /// `Hooks.LaserDraw.cpp`): refresh the copied endpoints from the live
    /// firer and target. A dead firer, or a TechnoType change under
    /// `StopOnFirerConvert`, stops the follow and leaves the last coordinate,
    /// as `OnObjectRemoved` nulls the retained pointer.
    pub fn update_tracking(&mut self, world: &Simulation, rules: &RuleSet) {
        let Some(track) = self.track.as_mut() else {
            return;
        };
        if let Some(id) = track.firer {
            let source = world.substrate.entities.get(id);
            let converted = track.stop_on_firer_convert
                && track.original_type.is_some_and(|original| {
                    source.is_none_or(|entity| entity.type_ref() != original)
                });
            if converted || source.is_none() {
                track.firer = None;
            } else if let Some((source, object)) = source.and_then(|source| {
                world
                    .interner
                    .object_type(source.type_ref(), rules)
                    .map(|object| (source, object))
            }) {
                let world_flh = super::fire_coord::fire_coordinate(
                    world,
                    rules,
                    &super::fire_coord::FireSource::of_entity(source),
                    object,
                    track.weapon_index,
                    track.burst_parity,
                    track.base_flh,
                )
                .coord;
                self.from = ProjectileCoord::new(
                    world_flh.x.wrapping_add(track.saved_offset.x),
                    world_flh.y.wrapping_add(track.saved_offset.y),
                    world_flh.z.wrapping_add(track.saved_offset.z),
                );
            }
        }
        if let Some(target) = track.target
            && let Some(to) = super::fire_coord::effect_target_coordinate(world, rules, target)
        {
            self.to = to;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaserColor {
    /// House+56FC is resolved once at construction by the existing client
    /// color owner. Capture the owner now, never look up the firer later.
    House(InternedId),
    /// The general54FE60 caller4A7526 copies the same House RGB and half
    /// of it for the outer lines. It does not select DrawBeamSpecial.
    HouseWithHalfOuter { owner: InternedId, spread: [u8; 3] },
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
    if super::world_fire::fire_at_effect(weapon) != super::world_fire::FireAtEffect::Laser {
        return;
    }
    let Some(source) = world.substrate.entities.get(source_id) else {
        return;
    };
    let Some(obj) = world.interner.object_type(source.type_ref(), rules) else {
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
    // FireAt6FF549..6FF571: every non-building firer's house-colour laser is
    // 2 wide (a LUNR on MoonPatr, which makes it buildable); a non-Prism
    // building keeps the constructor's 1.
    let width = if prism {
        if supported { 5 } else { 3 }
    } else if source.category() != EntityCategory::Structure && weapon.is_house_color {
        2
    } else {
        1
    };
    if let Some(laser) = create(
        world,
        rules,
        source_id,
        target.projectile_target(world),
        weapon_index,
        weapon,
        None,
        false,
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
/// supplied zero coordinate takes the same arm as `None`. `ignore_firer` is
/// Phobos' `ignoreShooter`: shrapnel's direct `CreateLaser` wrappers and
/// `Splits` children start from a detonation position and never follow the
/// firer, while the target end still tracks.
pub(crate) fn create<'w>(
    world: &'w mut Simulation,
    rules: &RuleSet,
    source_id: u64,
    target: ProjectileTarget,
    weapon_index: i32,
    weapon: &WeaponType,
    from: Option<ProjectileCoord>,
    ignore_firer: bool,
) -> Option<&'w mut LaserBirth> {
    let source = world.substrate.entities.get(source_id)?;
    #[cfg(feature = "phobos")]
    let single_color = weapon.phobos_laser_is_single_color;
    #[cfg(not(feature = "phobos"))]
    let single_color = false;
    let color = if weapon.is_house_color {
        LaserColor::House(source.owner())
    } else if single_color {
        // Phobos `TechnoClass_LaserZap_IsSingleColor` (`0x6FD446`) flips
        // `IsHouseColor` after the constructor; the special draw keeps the
        // stored inner colour and drops the outer pair.
        LaserColor::Explicit {
            inner: weapon.laser_inner_color,
            outer: [0; 3],
            spread: [0; 3],
        }
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
            let object = world.interner.object_type(source.type_ref(), rules)?;
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
                super::fire_coord::effect_z_adjust(
                    world,
                    source,
                    from,
                    super::fire_coord::EffectDepthProjection::ClampedBuilding,
                ),
            )
        };
    let to = super::fire_coord::effect_target_coordinate(world, rules, target)?;
    #[cfg(feature = "phobos")]
    let (follow, stop_on_firer_convert) = (
        weapon.laser_position_update,
        weapon
            .laser_position_update_stop_on_firer_convert
            .unwrap_or(rules.phobos_laser_tracking.stop_on_firer_convert),
    );
    #[cfg(not(feature = "phobos"))]
    let (follow, stop_on_firer_convert) = (PositionFollow::None, false);
    let track = build_track(
        world,
        rules,
        source_id,
        target,
        weapon_index,
        follow,
        stop_on_firer_convert,
        from,
        ignore_firer,
    );
    let birth = construct(
        &world.session,
        &mut world.lifecycle_outputs,
        from,
        to,
        z_adjust,
        weapon.laser_duration,
        color,
        NativeF32Bits::POSITIVE_ZERO,
    );
    birth.track = track;
    Some(birth)
}

/// Phobos `LaserRT::SetLaserTrackingData` / `TrackingData::Initialize`: the
/// objects one laser follows and the frozen `GetFLH` inputs that move its
/// source. `ignore_firer` is the shrapnel and split-child `ignoreShooter`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_track(
    world: &Simulation,
    rules: &RuleSet,
    source_id: u64,
    target: ProjectileTarget,
    weapon_index: i32,
    follow: PositionFollow,
    stop_on_firer_convert: bool,
    initial_from: ProjectileCoord,
    ignore_firer: bool,
) -> Option<LaserTrack> {
    if follow == PositionFollow::None {
        return None;
    }
    let source = world.substrate.entities.get(source_id);
    let object = source.and_then(|source| world.interner.object_type(source.type_ref(), rules));
    let burst_parity = source.map_or(0, |source| (source.weapon_burst.index() & 1) as u8);
    let mut firer = None;
    let mut base_flh = Flh::default();
    let mut saved_offset = ProjectileCoord::new(0, 0, 0);
    let mut original_type = None;
    // Initialize's building gate: a transport-capable building fires from an
    // occupant's port, so its laser never follows the building. An expired
    // firer falls through too, as `pShooter` arrives null there.
    if follow.follows_firer()
        && !ignore_firer
        && let Some(source) = source
        && let Some(object) = object
        && !(source.category() == EntityCategory::Structure && object.max_number_occupants > 0)
    {
        let use_elite = usize::try_from(weapon_index).is_ok_and(|index| {
            super::combat_weapon::uses_elite_weapon(object, source.veterancy.rank(), index)
        });
        base_flh = object.weapon_flh(weapon_index, use_elite);
        let world_flh = super::fire_coord::fire_coordinate(
            world,
            rules,
            &super::fire_coord::FireSource::of_entity(source),
            object,
            weapon_index,
            burst_parity,
            base_flh,
        )
        .coord;
        saved_offset = ProjectileCoord::new(
            initial_from.x.wrapping_sub(world_flh.x),
            initial_from.y.wrapping_sub(world_flh.y),
            initial_from.z.wrapping_sub(world_flh.z),
        );
        firer = Some(source_id);
        if stop_on_firer_convert {
            original_type = Some(source.type_ref());
        }
    }
    let target = (follow.follows_target()
        && matches!(
            target,
            ProjectileTarget::Entity(_) | ProjectileTarget::Terrain(_)
        ))
    .then_some(target);
    if firer.is_none() && target.is_none() {
        return None;
    }
    Some(LaserTrack {
        firer,
        target,
        weapon_index,
        burst_parity,
        base_flh,
        saved_offset,
        stop_on_firer_convert,
        original_type,
    })
}

/// Phobos `BulletExt::SimulatedFiringLaser`: the same copied birth from
/// explicit endpoints and the child's firing House, so a shot whose Techno
/// already expired still draws. `IsSingleColor`/`LaserThickness` replace the
/// stock color pair and width; `IsHouseColor` takes the House laser color.
#[cfg(feature = "phobos")]
pub(crate) fn create_effect<'w>(
    world: &'w mut Simulation,
    house: Option<InternedId>,
    from: ProjectileCoord,
    to: ProjectileCoord,
    weapon: &WeaponType,
) -> &'w mut LaserBirth {
    let (color, width, supported) = if weapon.is_house_color || weapon.phobos_laser_is_single_color
    {
        let color = match (weapon.is_house_color, house) {
            (true, Some(house)) => LaserColor::House(house),
            _ => LaserColor::Explicit {
                inner: weapon.laser_inner_color,
                outer: [0; 3],
                spread: [0; 3],
            },
        };
        let width = weapon.phobos_laser_thickness;
        (color, width, width > 3)
    } else {
        (
            LaserColor::Explicit {
                inner: weapon.laser_inner_color,
                outer: weapon.laser_outer_color,
                spread: weapon.laser_outer_spread,
            },
            3,
            false,
        )
    };
    let birth = construct(
        &world.session,
        &mut world.lifecycle_outputs,
        from,
        to,
        0,
        weapon.laser_duration,
        color,
        NativeF32Bits::POSITIVE_ZERO,
    );
    birth.width = width;
    birth.supported = supported;
    birth
}

/// LaserDraw54FE60 initializes every caller's width1/unsupported object;
/// caller-specific postprocessing mutates that one returned birth.
pub(crate) fn construct<'a>(
    session: &ScenarioSession,
    lifecycle_outputs: &'a mut FrameChannel<LifecycleOutput>,
    from: ProjectileCoord,
    to: ProjectileCoord,
    z_adjust: i32,
    duration: i32,
    color: LaserColor,
    fade_end: NativeF32Bits,
) -> &'a mut LaserBirth {
    let birth = lifecycle_outputs.emit_and_finish(LifecycleOutput::LaserCreated(LaserBirth {
        frame: session.binary_frame() as i32,
        from,
        to,
        z_adjust,
        duration,
        width: 1,
        supported: false,
        color,
        fade_end,
        track: None,
    }));
    let LifecycleOutput::LaserCreated(laser) = birth else {
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
    let Some(obj) = world.interner.object_type(source.type_ref(), rules) else {
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
        &world.session,
        &mut world.lifecycle_outputs,
        from,
        to,
        0,
        rules.general.prism_support.duration,
        color,
        NativeF32Bits::POSITIVE_ZERO,
    )
    .width = 3;
}
