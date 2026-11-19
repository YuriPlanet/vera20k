//! Copied EBolt construction inputs. Original6FD570/6FD460/4C2A60; the
//! app owns the non-Abstract tactical registry and its draw-count lifetime.
//!
//! A Unit's own bolt follows its firer: SpawnElectricBoltEffect
//! (`0x006FD5E2..0x006FD606`) names the live Unit as the bolt's owner
//! ([`ElectricBoltOwner`]) when its `+0x6DC` holds no bolt yet, and each
//! tactical pass (`EBoltMgr::UpdateAndDrawAll @ 0x004C2830`) moves the bolt's
//! source to the owner's current FLH ([`owner_source`]).

use crate::sim::projectile::ProjectileCoord;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElectricBoltBirth {
    pub frame: i32,
    pub from: ProjectileCoord,
    pub to: ProjectileCoord,
    pub z_adjust: i32,
    pub phase: i32,
    pub alternate_color: bool,
    /// Ares `Bolt.Color1..3` of the weapon that made the bolt
    /// (EBolt_Draw_Colors, Ares.dll 100561F0, hook 0x4C1F33): each set colour
    /// replaces the palette colour of the first outer, second outer and
    /// centre strand. Always `None` when Ares does not play.
    pub colors: [Option<[u8; 3]>; 3],
    /// Phobos `EBoltExt::Arcs` (`Ext/EBolt/Body.h:19`, constructor 8):
    /// `EBolt_DrawArcs` stops the subdivision loop once `plotIndex` reaches
    /// this count (`Ext/EBolt/Hooks.cpp:57..64`).
    pub arcs: i32,
    /// The firer [`fired`] names for `EBolt::SetOwner @ 0x004C2BD0`
    /// (bolt `+0x20`/`+0x24`); the app keeps it only while the Unit holds
    /// no other bolt (`+0x6DC`).
    pub owner: Option<ElectricBoltOwner>,
    /// `pBolt->Lifetime` (`EBoltExt::CreateEBolt`, `Ext/EBolt/Body.cpp:30`):
    /// `1 << (Bolt.Duration - 1)`, 0x10000 for the stock 17. The tactical
    /// pass shifts it right once per composite.
    pub decay: i32,
}

/// A bolt's owner and the weapon index its FLH is read for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElectricBoltOwner {
    pub unit: u64,
    pub weapon_index: i32,
    /// A Unit owner also occupies `Unit+0x6DC`, so only its first live bolt
    /// links; a non-Unit `Bolt.FollowFLH=yes` owner keeps every bolt in its
    /// `TechnoExt::ElectricBolts` vector (`Ext/EBolt/Hooks.cpp:104-121`).
    pub unit_slot: bool,
}

/// The source a bolt fired by `source`'s weapon `index` starts at: GetFLH
/// (vt+0xB0) with no base coordinate, as SpawnElectricBoltEffect asks it
/// (`0x006FD5A3`) and the tactical pass re-asks it for an owned bolt.
fn bolt_source(
    world: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    source: &crate::sim::game_entity::GameEntity,
    object: &crate::rules::object_type::ObjectType,
    index: i32,
) -> ProjectileCoord {
    super::fire_coord::fire_coordinate(
        world,
        rules,
        &super::fire_coord::FireSource::of_entity(source),
        object,
        index,
        source.weapon_burst.index() as u8,
        Default::default(),
    )
    .coord
}

/// `EBoltMgr::UpdateAndDrawAll @ 0x004C2830`'s owner arm: the owner's
/// current FLH for the bolt's weapon. `None` once the Unit is gone, which
/// the Unit destructor's ClearOwner (`0x00735789`) answers by detaching.
pub fn owner_source(
    world: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    owner: ElectricBoltOwner,
) -> Option<ProjectileCoord> {
    let source = world.substrate.entities.get(owner.unit)?;
    let object = world.interner.object_type(source.type_ref(), rules)?;
    Some(bolt_source(
        world,
        rules,
        source,
        object,
        owner.weapon_index,
    ))
}

/// FireAt6FF57D is subordinate to its IsLaser arm6FF4CC. Spawn6FD570 then
/// asks SelectWeapon and GetWeapon again after occupant/burst advancement;
/// that weapon supplies alternate color and the index passed to GetFLH.
pub(crate) fn fired(
    world: &mut crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    source_id: u64,
    target: super::TargetKind,
    fired_weapon: &crate::rules::weapon_type::WeaponType,
) {
    if super::world_fire::fire_at_effect(fired_weapon)
        != super::world_fire::FireAtEffect::ElectricBolt
    {
        return;
    }
    let Some(source) = world.substrate.entities.get(source_id) else {
        return;
    };
    let Some(object) = world.interner.object_type(source.type_ref(), rules) else {
        return;
    };
    let Some((index, Some(selected))) = super::combat_weapon::select_weapon_against(
        rules,
        source,
        object,
        &super::combat_weapon::attacker_facts(source, object),
        Some(&target),
        world,
        &world.interner,
        world.resolved_terrain.as_ref(),
        Some(&world.house_alliances),
    ) else {
        return;
    };
    let from = bolt_source(world, rules, source, object, index);
    // Phobos `EBoltExt::_SetOwner` (`Ext/EBolt/Hooks.cpp:104-121`): the
    // weapon's `Bolt.FollowFLH` overrides the stock Unit-only link, and the
    // hook bypasses the native SetOwner checks at `0x4C2BF2` (InLimbo) and
    // the Unit `+0x6DC` slot test at `0x006FD5F2`..`0x006FD600`, so every
    // linked bolt follows and a Limbo Unit links as well.
    #[cfg(feature = "phobos")]
    let follow = selected
        .weapon
        .phobos_bolt_follow_flh
        .unwrap_or(source.category() == crate::map::entities::EntityCategory::Unit);
    #[cfg(not(feature = "phobos"))]
    let follow = source.category() == crate::map::entities::EntityCategory::Unit;
    #[cfg(feature = "phobos")]
    let link = follow && source.is_object_alive();
    #[cfg(not(feature = "phobos"))]
    let link = follow && source.is_object_alive() && !source.in_limbo();
    let owner = link.then_some(ElectricBoltOwner {
        unit: source_id,
        weapon_index: index,
        unit_slot: source.category() == crate::map::entities::EntityCategory::Unit,
    });
    #[cfg(feature = "phobos")]
    let visual_scatter = selected.weapon.phobos_visual_scatter();
    #[cfg(not(feature = "phobos"))]
    let visual_scatter = false;
    create(
        world,
        rules,
        source_id,
        target.projectile_target(world),
        selected.weapon,
        from,
        owner,
        visual_scatter,
    );
}

/// TechnoClass::CreateElectricBolt6FD460 shared by FireAt's reselecting
/// wrapper and direct SpawnShrapnel46A8CC/46ADB9 calls. The caller owns
/// IsElectricBolt/IsLaser gating; this body uses its explicit weapon/source.
/// `visual_scatter` is Phobos `VisualScatter=`: only `FireAt`'s own
/// CreateEBolt call sets it, because the hook is gated on
/// `FireAtTemp::FireBullet`, which a shrapnel or split-child draw lacks.
pub(crate) fn create(
    world: &mut crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    source_id: u64,
    target: crate::sim::projectile::ProjectileTarget,
    weapon: &crate::rules::weapon_type::WeaponType,
    from: ProjectileCoord,
    owner: Option<ElectricBoltOwner>,
    visual_scatter: bool,
) {
    let Some(source) = world.substrate.entities.get(source_id) else {
        return;
    };
    #[cfg(not(feature = "phobos"))]
    let _ = visual_scatter;
    #[cfg(feature = "phobos")]
    let clamp_positive = crate::rules::phobos_beam_z_adjust::resolved_clamp(
        weapon.phobos_ebolt_clamp_initial_depth_for_building,
        rules
            .phobos_beam_z_adjust
            .ebolt_clamp_initial_depth_for_building,
    );
    #[cfg(not(feature = "phobos"))]
    let clamp_positive = true;
    let z_adjust = super::fire_coord::effect_z_adjust(
        world,
        source,
        from,
        super::fire_coord::EffectDepthProjection::ClampedBuilding,
        clamp_positive,
    );
    let Some(mut to) = super::fire_coord::effect_target_coordinate(world, rules, target) else {
        return;
    };
    // Phobos `TechnoClass_DrawSth_DrawToInvisoFlakScatterLocation`
    // (`0x6FD514`, `Ext/Techno/Hooks.WeaponEffects.cpp:220-248`). CreateEBolt
    // hands the same coordinate to the bolt and to its spark system, so the
    // replacement has to happen before `emit`.
    #[cfg(feature = "phobos")]
    if visual_scatter
        && let Some(scattered) = super::fire_coord::visual_scatter_target(world, rules, target)
    {
        to = scattered;
    }
    // Ares TechnoClass_FireEBolt (Ares.dll 10056350, hook 0x6FD469) creates
    // this body's bolt through CreateBolt, which registers the weapon's
    // WeaponTypeExt for the bolt hooks.
    emit(
        world,
        rules,
        from,
        to,
        z_adjust,
        weapon,
        owner,
        rules.ares(),
    );
}

/// Phobos `BulletExt::SimulatedFiringElectricBolt`: explicit endpoints and no
/// live source Techno, so an airburst/split child still draws after its firer
/// expired.
#[cfg(feature = "phobos")]
pub(crate) fn create_effect(
    world: &mut crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    from: ProjectileCoord,
    to: ProjectileCoord,
    weapon: &crate::rules::weapon_type::WeaponType,
    owner: Option<ElectricBoltOwner>,
) {
    // The birth (`emit`) adds `EBoltZAdjust` once; a simulated bolt starts
    // from zero depth (`Ext/Bullet/Body.cpp:268-280`).
    let z_adjust = 0;
    emit(world, rules, from, to, z_adjust, weapon, owner, false);
}

/// `ares_ext` is whether the bolt carries the weapon's Ares WeaponTypeExt.
#[allow(clippy::too_many_arguments)]
fn emit(
    world: &mut crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    from: ProjectileCoord,
    to: ProjectileCoord,
    z_adjust: i32,
    weapon: &crate::rules::weapon_type::WeaponType,
    owner: Option<ElectricBoltOwner>,
    ares_ext: bool,
) {
    let alternate_color = weapon.is_alternate_color;
    // Phobos `EBolt_Draw_*` (`Ext/EBolt/Hooks.cpp:215`): the weapon's
    // EBoltZAdjust else `[AudioVisual] EBoltZAdjust` adds to the bolt's
    // zAdjust before the birth is retained.
    #[cfg(feature = "phobos")]
    let z_adjust = z_adjust.wrapping_add(crate::rules::phobos_beam_z_adjust::resolved(
        weapon.phobos_ebolt_z_adjust,
        rules.phobos_beam_z_adjust.ebolt,
    ));
    let visuals = ares_ext.then_some(&weapon.ares_visuals);
    let colors = visuals.map_or([None; 3], |visuals| visuals.bolt_colors);
    // Init4C2AA3 draws from process Main, before constructing SparkSys. The
    // shared RNG owns every rejection retry. EBolt itself allocates no ID.
    let phase = world.main_rng.next_range_i32_inclusive(0, 256);
    // `EBoltExt::CreateEBolt` (`Ext/EBolt/Body.cpp:30`): the constructor's
    // Lifetime is the weapon's `Bolt.Duration` exponent, default 17.
    #[cfg(feature = "phobos")]
    let decay = 1_i32 << (weapon.phobos_bolt_duration.clamp(1, 31) - 1);
    #[cfg(not(feature = "phobos"))]
    let decay = 0x10000;
    // `EBoltExt::CreateEBolt` copies the weapon's `Bolt.Arcs`
    // (`Ext/EBolt/Body.cpp:29`); the constructor's 8 is the default.
    #[cfg(feature = "phobos")]
    let arcs = weapon.phobos_bolt_arcs;
    #[cfg(not(feature = "phobos"))]
    let arcs = 8;
    world
        .lifecycle_outputs
        .emit(crate::sim::world::LifecycleOutput::ElectricBoltCreated(
            ElectricBoltBirth {
                frame: world.session.binary_frame() as i32,
                from,
                to,
                z_adjust,
                phase,
                alternate_color,
                colors,
                arcs,
                owner,
                decay,
            },
        ));
    // Init4C2B30 supplies the copied TARGET, no owner/attachment and static
    // zero aim8A0E50; SetOwner follows only for FireAt's Unit bolt. The
    // constructor alone owns ID allocation and live Logic tail
    // registration. A preceding Inviso Bullet can remove itself
    // and compact this entry behind the advancing cursor, deferring its AI
    // to the next pass (electric_bolt.json's live scheduling controls).
    let default_system = || rules.combat_damage.default_spark_system.as_deref();
    // Ares EBolt_Fire_Particles (Ares.dll 100562C0, hook 0x4C2AFF): a bolt
    // with WeaponTypeExt spawns Bolt.ParticleSystem instead, with the same
    // arguments; an explicit `none` spawns nothing.
    let system = match visuals.and_then(|visuals| visuals.bolt_particle_system.as_ref()) {
        Some(system) => system.as_deref(),
        None => default_system(),
    };
    if let Some(system_type) = system.and_then(|name| rules.ps_type_id_by_name(name)) {
        world.spawn_particle_system(
            system_type,
            glam::IVec3::new(to.x, to.y, to.z),
            None,
            None,
            glam::IVec3::ZERO,
            None,
            rules,
        );
    }
}
