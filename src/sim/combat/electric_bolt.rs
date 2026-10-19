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
    /// The firer [`fired`] names for `EBolt::SetOwner @ 0x004C2BD0`
    /// (bolt `+0x20`/`+0x24`); the app keeps it only while the Unit holds
    /// no other bolt (`+0x6DC`).
    pub owner: Option<ElectricBoltOwner>,
}

/// A bolt's owner Unit and the weapon index its FLH is read for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElectricBoltOwner {
    pub unit: u64,
    pub weapon_index: i32,
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
        (source.weapon_burst.index() & 1) as u8,
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
    // `0x006FD5E2..0x006FD5FA` admits a live Unit (its `+0x6DC` slot is the
    // app registry's); SetOwner (`0x004C2BF2`) then refuses one in Limbo.
    // RESIDUAL: native has already stored the bolt in `+0x6DC` by then
    // (`0x006FD600`), so a Limbo Unit's slot stays taken by an unowned bolt
    // and its later bolts never follow; here the slot stays free. Trigger: a
    // Limbo Unit firing an IsElectricBolt weapon; no retail case is known.
    let owner = (source.category() == crate::map::entities::EntityCategory::Unit
        && source.is_object_alive()
        && !source.in_limbo())
    .then_some(ElectricBoltOwner {
        unit: source_id,
        weapon_index: index,
    });
    create(
        world,
        rules,
        source_id,
        target.projectile_target(world),
        selected.weapon,
        from,
        owner,
    );
}

/// TechnoClass::CreateElectricBolt6FD460 shared by FireAt's reselecting
/// wrapper and direct SpawnShrapnel46A8CC/46ADB9 calls. The caller owns
/// IsElectricBolt/IsLaser gating; this body uses its explicit weapon/source.
pub(crate) fn create(
    world: &mut crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    source_id: u64,
    target: crate::sim::projectile::ProjectileTarget,
    weapon: &crate::rules::weapon_type::WeaponType,
    from: ProjectileCoord,
    owner: Option<ElectricBoltOwner>,
) {
    let Some(source) = world.substrate.entities.get(source_id) else {
        return;
    };
    let z_adjust = super::fire_coord::effect_z_adjust(
        world,
        source,
        from,
        super::fire_coord::EffectDepthProjection::ClampedBuilding,
    );
    let Some(to) = super::fire_coord::effect_target_coordinate(world, rules, target) else {
        return;
    };
    let alternate_color = weapon.is_alternate_color;
    // Init4C2AA3 draws from process Main, before constructing SparkSys. The
    // shared RNG owns every rejection retry. EBolt itself allocates no ID.
    let phase = world.main_rng.next_range_i32_inclusive(0, 256);
    world
        .lifecycle_outputs
        .push(crate::sim::world::LifecycleOutput::ElectricBoltCreated(
            ElectricBoltBirth {
                frame: world.session.binary_frame() as i32,
                from,
                to,
                z_adjust,
                phase,
                alternate_color,
                owner,
            },
        ));
    // Init4C2B30 supplies the copied TARGET, no owner/attachment and static
    // zero aim8A0E50; SetOwner follows only for FireAt's Unit bolt. The
    // constructor alone owns ID allocation and live Logic tail
    // registration. A preceding Inviso Bullet can remove itself
    // and compact this entry behind the advancing cursor, deferring its AI
    // to the next pass (electric_bolt.json's live scheduling controls).
    if let Some(system_type) = rules
        .combat_damage
        .default_spark_system
        .as_deref()
        .and_then(|name| rules.ps_type_id_by_name(name))
    {
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
