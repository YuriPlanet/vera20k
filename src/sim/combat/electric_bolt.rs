//! Copied EBolt construction inputs. Original6FD570/6FD460/4C2A60; the
//! app owns the non-Abstract tactical registry and its draw-count lifetime.

use crate::sim::projectile::ProjectileCoord;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElectricBoltBirth {
    pub frame: i32,
    pub from: ProjectileCoord,
    pub to: ProjectileCoord,
    pub z_adjust: i32,
    pub phase: i32,
    pub alternate_color: bool,
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
    if fired_weapon.is_laser || !fired_weapon.is_electric_bolt {
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
        Some(&world.fog.alliances),
    ) else {
        return;
    };
    let from = super::fire_coord::fire_coordinate(
        world,
        rules,
        &super::fire_coord::FireSource::of_entity(source),
        object,
        index,
        (source.weapon_burst.index() & 1) as u8,
        Default::default(),
    )
    .coord;
    create(
        world,
        rules,
        source_id,
        target.projectile_target(world),
        selected.weapon,
        from,
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
                frame: world.session.binary_frame as i32,
                from,
                to,
                z_adjust,
                phase,
                alternate_color,
            },
        ));
    // Init4C2B30 supplies the copied TARGET, no owner/attachment and static
    // zero aim8A0E50. The constructor alone owns ID allocation and live
    // Logic tail registration. A preceding Inviso Bullet can remove itself
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
