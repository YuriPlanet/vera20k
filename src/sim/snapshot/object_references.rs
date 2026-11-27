//! Object identity and pointer validation for a loaded snapshot: the one
//! restored-object index over every store, and the pass that requires each
//! saved pointer slot to resolve to an object of an admitted kind while
//! clearing only deliberate weak identities.

// Outside data reaches this module: it refuses bad input, never panics.
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::unreachable,
        clippy::indexing_slicing
    )
)]

use std::collections::{BTreeMap, BTreeSet};

use super::SnapshotRestoreError;
use crate::sim::world::Simulation;

#[derive(Debug, Clone, Copy)]
pub(super) struct RestoredObjectIndex {
    pub(super) highest_id: u64,
}

impl RestoredObjectIndex {
    pub(super) fn build(
        sim: &Simulation,
    ) -> Result<(Self, BTreeMap<u64, &'static str>), SnapshotRestoreError> {
        use crate::sim::world::ObjectKind;
        let mut identities = BTreeMap::new();
        let registry = &mut identities;
        for kind in ObjectKind::ALL {
            match kind {
                ObjectKind::Entity => {
                    for (id, entity) in sim.substrate.entities.iter_sorted() {
                        Self::register(registry, "EntityStore", id, entity.stable_id())?;
                    }
                }
                ObjectKind::Anim => {
                    for (&id, anim) in sim.substrate.anims.iter() {
                        Self::register(registry, "AnimStore", id, anim.stable_id())?;
                    }
                }
                ObjectKind::VoxelAnim => {
                    for (&id, debris) in sim.substrate.voxel_anims.iter() {
                        Self::register(registry, "VoxelAnimStore", id, debris.stable_id())?;
                    }
                }
                ObjectKind::Spotlight => {
                    for (&id, light) in sim.substrate.spotlights.iter() {
                        Self::register(registry, "SpotlightStore", id, light.stable_id())?;
                    }
                }
                ObjectKind::ParticleSystem => {
                    for (&id, system) in sim.substrate.particle_systems.iter() {
                        Self::register(registry, "ParticleSystemStore", id, system.stable_id())?;
                    }
                }
                // Individual particles retain AIR Display membership.
                ObjectKind::Particle => {
                    for (_, system) in sim.substrate.particle_systems.iter() {
                        for particle in system.particles() {
                            let id = particle.stable_id();
                            Self::register(registry, "ParticleSystemStore.children", id, id)?;
                        }
                    }
                    for (id, particle, _, _) in sim.substrate.particle_systems.detached_particles()
                    {
                        Self::register(
                            registry,
                            "ParticleSystemStore.detached",
                            id,
                            particle.stable_id(),
                        )?;
                    }
                }
                ObjectKind::Terrain => {
                    for (&id, terrain) in sim.terrain_objects.objects() {
                        Self::register(registry, "TerrainObjectStore", id, terrain.stable_id())?;
                    }
                }
                ObjectKind::Projectile => {
                    for (&id, projectile) in sim.projectiles.iter() {
                        Self::register(registry, "ProjectileStore", id, projectile.id())?;
                    }
                }
                ObjectKind::Wave => {
                    for (&id, wave) in sim.waves.iter() {
                        Self::register(registry, "WaveStore", id, wave.id())?;
                    }
                }
                ObjectKind::Smudge => {
                    for (id, object) in sim.smudge_grid.iter().flat_map(|grid| grid.objects()) {
                        Self::register(registry, "SmudgeGrid", id, object.stable_id())?;
                    }
                }
            }
        }
        // The other registries numbered from the same allocator.
        for shot in sim.disk_laser_observations() {
            Self::register(registry, "DiskLaserStore", shot.id, shot.id)?;
        }
        for id in sim.load_objects.retained_overlay_ids() {
            Self::register(registry, "LoadObjectLifecycle", id, id)?;
        }

        let highest_id = identities.last_key_value().map_or(0, |(&id, _)| id);
        Ok((Self { highest_id }, identities))
    }

    fn register(
        identities: &mut BTreeMap<u64, &'static str>,
        registry: &'static str,
        registry_id: u64,
        object_id: u64,
    ) -> Result<(), SnapshotRestoreError> {
        if registry_id == 0 {
            return Err(SnapshotRestoreError::ReservedObjectId { registry });
        }
        if registry_id != object_id {
            return Err(SnapshotRestoreError::ObjectIdentityMismatch {
                registry,
                registry_id,
                object_id,
            });
        }
        if let Some(first_registry) = identities.insert(registry_id, registry) {
            return Err(SnapshotRestoreError::DuplicateObjectIdentity {
                object_id: registry_id,
                first_registry,
                second_registry: registry,
            });
        }
        Ok(())
    }
}

fn require_resolved_reference(
    resolves: bool,
    source_registry: &'static str,
    source_id: u64,
    field: &'static str,
    target_registry: &'static str,
    target_id: u64,
) -> Result<(), SnapshotRestoreError> {
    if resolves {
        Ok(())
    } else {
        Err(SnapshotRestoreError::UnresolvedObjectReference {
            source_registry,
            source_id,
            field,
            target_registry,
            target_id,
        })
    }
}

/// The one rule for the stores an Object pointer may name, whether TarCom,
/// NavCom or another target or owner slot: a Techno or a Terrain, the two
/// stores whose objects the coordinate owners (`target_get_coords`,
/// `nav_target_coordinate`) resolve.
fn require_object_reference(
    entity_ids: &BTreeSet<u64>,
    terrain_objects: &BTreeMap<u64, crate::sim::terrain_object::TerrainObjectState>,
    source_registry: &'static str,
    source_id: u64,
    field: &'static str,
    target_id: u64,
) -> Result<(), SnapshotRestoreError> {
    require_resolved_reference(
        entity_ids.contains(&target_id) || terrain_objects.contains_key(&target_id),
        source_registry,
        source_id,
        field,
        "EntityStore or TerrainObjectStore",
        target_id,
    )
}

pub(super) fn validate_passenger_size_tables(sim: &Simulation) -> Result<(), SnapshotRestoreError> {
    for (carrier_id, entity) in sim.substrate.entities.iter_sorted() {
        let Some(cargo) = entity.passenger_role().cargo() else {
            continue;
        };
        if cargo.passengers().len() != cargo.passenger_sizes().len() {
            return Err(SnapshotRestoreError::PassengerSizeCountMismatch {
                carrier_id,
                passenger_count: cargo.passengers().len(),
                size_count: cargo.passenger_sizes().len(),
            });
        }
        let computed_total = cargo.recomputed_total_size();
        if computed_total != cargo.total_size() {
            return Err(SnapshotRestoreError::PassengerSizeTotalMismatch {
                carrier_id,
                saved_total: cargo.total_size(),
                computed_total,
            });
        }
    }
    Ok(())
}

pub(super) fn restore_object_references(
    sim: &mut Simulation,
    identities: &BTreeMap<u64, &'static str>,
) -> Result<(), SnapshotRestoreError> {
    use crate::sim::combat::TargetKind;
    use crate::sim::docking::bunker_link::BunkerLink;
    use crate::sim::projectile::ProjectileTarget;

    let entity_ids: BTreeSet<u64> = sim.substrate.entities.keys_sorted().into_iter().collect();
    let anim_ids: BTreeSet<u64> = sim.substrate.anims.iter().map(|(&id, _)| id).collect();
    let particle_system_ids: BTreeSet<u64> = sim
        .substrate
        .particle_systems
        .iter()
        .map(|(&id, _)| id)
        .collect();

    if let Some(id) = sim.particle_systems().gas_cloud_system() {
        require_resolved_reference(
            particle_system_ids.contains(&id),
            "ParticleSystemStore",
            0,
            "gas_cloud_system",
            "ParticleSystemStore",
            id,
        )?;
    }

    // Building saved+55C slots swizzle to distinct owned Anim objects. Validate
    // before weak-pointer cleanup or reverse-index mutation. Anim+CC may remain
    // null, and a deferred Destroy is still a physically present valid target.
    let mut claimed_slots = BTreeMap::new();
    for (owner_id, entity) in sim.substrate.entities.iter_sorted() {
        // Diagnostic slots0..20 are Building+55C, slots21..28 are +5C8.
        for (slot, anim_id) in entity
            .building_art
            .anim_slots()
            .iter()
            .chain(entity.damage_fires.anim_ids().iter())
            .enumerate()
        {
            let Some(anim_id) = *anim_id else { continue };
            let slot = slot as u8;
            if entity.category() != crate::map::entities::EntityCategory::Structure {
                return Err(SnapshotRestoreError::InvalidBuildingAnimSlotOwner { owner_id, slot });
            }
            if !anim_ids.contains(&anim_id) {
                return Err(SnapshotRestoreError::MissingBuildingSlotAnim {
                    owner_id,
                    slot,
                    anim_id,
                });
            }
            if let Some((first_owner, first_slot)) = claimed_slots.insert(anim_id, (owner_id, slot))
            {
                return Err(SnapshotRestoreError::DuplicateBuildingSlotAnim {
                    anim_id,
                    first_owner,
                    first_slot,
                    second_owner: owner_id,
                    second_slot: slot,
                });
            }
        }
    }

    // Swizzle::Apply has no unmatched-reference recovery path. Validate the
    // complete modeled pointer graph before mutating even weak references or
    // derived caches, so a failed restore remains an atomic rejection.
    for (entity_id, entity) in sim.substrate.entities.iter_sorted() {
        // UnitClass::Load744470 queues +6C8 for swizzling (gamemd.exe).
        // Preserve the independent +6D0 byte, cycles and physically present
        // dead/limbo Units; only class identity and reference resolution apply.
        if (entity.unit_follower().is_some() || entity.is_unit_follower())
            && entity.category() != crate::map::entities::EntityCategory::Unit
        {
            return Err(SnapshotRestoreError::InvalidUnitFollowerState {
                object_id: entity_id,
                reason: "the relationship and follower byte belong to a Unit",
            });
        }
        if let Some(follower_id) = entity.unit_follower() {
            require_resolved_reference(
                entity_ids.contains(&follower_id),
                "EntityStore",
                entity_id,
                "unit_follower",
                "EntityStore",
                follower_id,
            )?;
            if sim
                .substrate
                .entities
                .get(follower_id)
                .is_none_or(|follower| {
                    follower.category() != crate::map::entities::EntityCategory::Unit
                })
            {
                return Err(SnapshotRestoreError::InvalidUnitFollowerState {
                    object_id: entity_id,
                    reason: "the follower reference must name a Unit",
                });
            }
        }
        if let Some(anim_id) = entity
            .parasite
            .class()
            .and_then(|class| class.grapple_anim())
        {
            require_resolved_reference(
                anim_ids.contains(&anim_id),
                "EntityStore",
                entity_id,
                "parasite.grapple.anim",
                "AnimStore",
                anim_id,
            )?;
        }
        if entity.seldom.tesla_charger.is_active()
            && !matches!(
                entity.category(),
                crate::map::entities::EntityCategory::Unit
                    | crate::map::entities::EntityCategory::Infantry
                    | crate::map::entities::EntityCategory::Aircraft
            )
        {
            return Err(SnapshotRestoreError::InvalidTeslaChargingState {
                object_id: entity_id,
                reason: "charging intent belongs to a Foot",
            });
        }
        if !entity.building.tesla_charge.is_default()
            && entity.category() != crate::map::entities::EntityCategory::Structure
        {
            return Err(SnapshotRestoreError::InvalidTeslaChargingState {
                object_id: entity_id,
                reason: "charger list and weapon latch belong to a Building",
            });
        }
        let mut seen_chargers = BTreeSet::new();
        for &charger in entity.building.tesla_charge.chargers() {
            require_resolved_reference(
                entity_ids.contains(&charger),
                "EntityStore",
                entity_id,
                "tesla_charge.chargers",
                "EntityStore",
                charger,
            )?;
            if !seen_chargers.insert(charger)
                || sim.substrate.entities.get(charger).is_none_or(|charger| {
                    charger.category() != crate::map::entities::EntityCategory::Infantry
                })
            {
                return Err(SnapshotRestoreError::InvalidTeslaChargingState {
                    object_id: entity_id,
                    reason: "chargers must be distinct Infantry identities",
                });
            }
        }
        // Do not require a current reciprocal TarCom or recompute +661:
        // native saves can lie between retarget/expiry and Building AI.
        for contact_id in entity.seldom.radio_contacts.iter_live() {
            require_resolved_reference(
                entity_ids.contains(&contact_id),
                "EntityStore",
                entity_id,
                "radio_contacts",
                "EntityStore",
                contact_id,
            )?;
        }

        if let Some(cargo) = entity.passenger_role().cargo() {
            for &passenger_id in cargo.passengers() {
                require_resolved_reference(
                    entity_ids.contains(&passenger_id),
                    "EntityStore",
                    entity_id,
                    "passenger_role.cargo.passengers",
                    "EntityStore",
                    passenger_id,
                )?;
            }
        }

        // BuildingClass's saved vector+684 contains independent Infantry
        // references; Cargo+114 is validated above, never reclassified by flags.
        for &occupant_id in entity.passenger_role().occupants().ids() {
            require_resolved_reference(
                entity_ids.contains(&occupant_id),
                "EntityStore",
                entity_id,
                "passenger_role.occupants.ids",
                "EntityStore",
                occupant_id,
            )?;
        }

        // SpawnManagerClass__Load @ 0x006B7F10 queues every non-null
        // SpawnControl child pointer plus its current and queued target slots
        // for the common post-load swizzle pass. Rust keeps stable IDs in those
        // fields, so admission requires the same references to resolve before
        // any cleanup mutates the restored graph.
        if let Some(owner) = entity.airstrike_link() {
            require_resolved_reference(
                sim.substrate
                    .entities
                    .get(owner)
                    .is_some_and(|owner| owner.seldom.airstrike.owns_manager()),
                "EntityStore",
                entity_id,
                "airstrike.link",
                "AirstrikeManager",
                owner,
            )?;
        }
        for referenced_id in entity.seldom.airstrike.referenced_ids() {
            require_resolved_reference(
                entity_ids.contains(&referenced_id),
                "EntityStore",
                entity_id,
                "airstrike.reference",
                "EntityStore",
                referenced_id,
            )?;
        }
        if let Some(manager) = entity.spawn_manager.as_ref() {
            for slot in manager.slots() {
                if let Some(spawn_id) = slot.spawn() {
                    require_resolved_reference(
                        entity_ids.contains(&spawn_id),
                        "EntityStore",
                        entity_id,
                        "spawn_manager.slots.spawn",
                        "EntityStore",
                        spawn_id,
                    )?;
                }
            }
            if let Some(TargetKind::Entity(target_id)) = manager.current_target() {
                require_object_reference(
                    &entity_ids,
                    sim.terrain_objects.objects(),
                    "EntityStore",
                    entity_id,
                    "spawn_manager.current_target",
                    target_id,
                )?;
            }
            if let Some(TargetKind::Entity(target_id)) = manager.queued_target() {
                require_object_reference(
                    &entity_ids,
                    sim.terrain_objects.objects(),
                    "EntityStore",
                    entity_id,
                    "spawn_manager.queued_target",
                    target_id,
                )?;
            }
        }

        // TechnoClass__Load @ 0x0070BF50 queues the spawned-child parent
        // pointer at Techno+0x2D4. It is an independently swizzled slot: native
        // load does not require a reciprocal SpawnControl entry.
        if let Some(parent_id) = entity.seldom.spawn_owner_id.owner() {
            require_resolved_reference(
                entity_ids.contains(&parent_id),
                "EntityStore",
                entity_id,
                "spawn_owner_id",
                "EntityStore",
                parent_id,
            )?;
        }

        // A bomb's planter (`BombClass+0x24`) is a pointer its expiry nulls
        // (`BombListClass::PointerGotInvalid @ 0x00439150`).
        if let Some(planter) = entity.bomb.as_ref().and_then(|bomb| bomb.planter()) {
            require_resolved_reference(
                entity_ids.contains(&planter),
                "EntityStore",
                entity_id,
                "bomb.planter",
                "EntityStore",
                planter,
            )?;
        }

        if let Some(TargetKind::Entity(target_id)) = entity.attack_target.target() {
            require_object_reference(
                &entity_ids,
                sim.terrain_objects.objects(),
                "EntityStore",
                entity_id,
                "attack_target",
                target_id,
            )?;
        }
        if let Some(TargetKind::Entity(target_id)) = entity.seldom.suspended_attack_target.target()
        {
            require_object_reference(
                &entity_ids,
                sim.terrain_objects.objects(),
                "EntityStore",
                entity_id,
                "suspended_attack_target",
                target_id,
            )?;
        }

        let nav = &entity.navigation;
        for (field, targets) in [
            (
                "navigation.suspended_nav_com",
                nav.suspended_nav_com().as_slice(),
            ),
            ("navigation.nav_com_aux", nav.nav_com_aux().as_slice()),
            ("navigation.nav_com", nav.nav_com().as_slice()),
            ("navigation.nav_queue", nav.nav_queue()),
            ("navigation.nav_list", nav.nav_list()),
        ] {
            for target_id in targets.iter().filter_map(|target| target.object_id()) {
                require_object_reference(
                    &entity_ids,
                    sim.terrain_objects.objects(),
                    "EntityStore",
                    entity_id,
                    field,
                    target_id,
                )?;
            }
        }

        if let Some(target_id) = entity.seldom.tether.partner() {
            require_resolved_reference(
                entity_ids.contains(&target_id),
                "EntityStore",
                entity_id,
                "tether",
                "EntityStore",
                target_id,
            )?;
        }

        if let Some(manager) = entity.capture_manager.as_ref() {
            for target_id in manager.victims() {
                require_resolved_reference(
                    entity_ids.contains(&target_id),
                    "EntityStore",
                    entity_id,
                    "capture_manager.nodes",
                    "EntityStore",
                    target_id,
                )?;
            }
        }
        if let Some(controller_id) = entity.seldom.mind_control.controller() {
            require_resolved_reference(
                entity_ids.contains(&controller_id),
                "EntityStore",
                entity_id,
                "mind_control.controller",
                "EntityStore",
                controller_id,
            )?;
        }
        for target_id in entity.temporal.references() {
            require_resolved_reference(
                entity_ids.contains(&target_id),
                "EntityStore",
                entity_id,
                "temporal",
                "EntityStore",
                target_id,
            )?;
        }
        if let Some(pending) = entity.pending_entry() {
            require_resolved_reference(
                entity_ids.contains(&pending),
                "EntityStore",
                entity_id,
                "pending_entry",
                "EntityStore",
                pending,
            )?;
        }
        if let Some(ammo) = entity.ammo()
            && let Some(target_id) = ammo.dock()
        {
            require_resolved_reference(
                entity_ids.contains(&target_id),
                "EntityStore",
                entity_id,
                "ammo.dock",
                "EntityStore",
                target_id,
            )?;
        }
        if let Some(master_id) = entity.seldom.slave.owner() {
            require_resolved_reference(
                entity_ids.contains(&master_id),
                "EntityStore",
                entity_id,
                "slave.owner",
                "EntityStore",
                master_id,
            )?;
        }
        if let Some(manager) = entity.slave_manager.as_ref() {
            for slave_id in manager.slaves() {
                require_resolved_reference(
                    entity_ids.contains(&slave_id),
                    "EntityStore",
                    entity_id,
                    "slave_manager.nodes.slave",
                    "EntityStore",
                    slave_id,
                )?;
            }
        }

        if let Some(target_id) = entity.passenger_role().inside_transport_id() {
            require_resolved_reference(
                entity_ids.contains(&target_id),
                "EntityStore",
                entity_id,
                "passenger_role.inside",
                "EntityStore",
                target_id,
            )?;
        }

        for (field, system_id) in [
            (
                "fire_particle_system",
                entity.seldom.fire_particle_system.system(),
            ),
            (
                "spark_particle_system",
                entity.seldom.spark_particle_system.system(),
            ),
            (
                "railgun_particle_system",
                entity.seldom.railgun_particle_system.system(),
            ),
            (
                "damage_smoke_system_id",
                entity.seldom.damage_smoke_system_id.system(),
            ),
            (
                "natural_particle_system",
                entity.building.natural_particle_system.system(),
            ),
            (
                "destroy_smoke_particle_system",
                entity.building.destroy_smoke_particle_system.system(),
            ),
        ]
        .into_iter()
        .filter_map(|(field, system)| system.map(|id| (field, id)))
        {
            require_resolved_reference(
                particle_system_ids.contains(&system_id),
                "EntityStore",
                entity_id,
                field,
                "ParticleSystemStore",
                system_id,
            )?;
        }
        if let Some(target_id) = entity.bunker.occupant() {
            require_resolved_reference(
                entity_ids.contains(&target_id),
                "EntityStore",
                entity_id,
                "bunker_occupant",
                "EntityStore",
                target_id,
            )?;
        }
        if let BunkerLink::Installed(target_id) = entity.bunker.link() {
            require_resolved_reference(
                entity_ids.contains(&target_id),
                "EntityStore",
                entity_id,
                "bunker_link",
                "EntityStore",
                target_id,
            )?;
        }
    }

    for (&anim_id, anim) in sim.substrate.anims.iter() {
        if let Some(owner_id) = anim.owner_entity() {
            require_resolved_reference(
                entity_ids.contains(&owner_id),
                "AnimStore",
                anim_id,
                "owner_entity",
                "EntityStore",
                owner_id,
            )?;
        }
    }

    for (&system_id, system) in sim.substrate.particle_systems.iter() {
        // A VoxelAnim owns its AttachedSystem (`0x007498A5`); Smoke follows
        // it through `target_get_coords`, which resolves that store too.
        if let Some(owner_id) = system
            .owner_entity()
            .filter(|&owner| !sim.substrate.voxel_anims.contains_key(owner))
        {
            require_object_reference(
                &entity_ids,
                sim.terrain_objects.objects(),
                "ParticleSystemStore",
                system_id,
                "owner_entity",
                owner_id,
            )?;
        }
        if let Some(crate::sim::combat::TargetKind::Entity(attached_id)) = system.target_ref() {
            require_object_reference(
                &entity_ids,
                sim.terrain_objects.objects(),
                "ParticleSystemStore",
                system_id,
                "target_ref",
                attached_id,
            )?;
        }
    }

    // BulletClass__Load @ 0x0046AE70 queues the non-null source/firer pointer
    // at +0xB0, target at +0x10C and awaited Anim at +0x154 for global
    // swizzling (46AF8B), independently of the wait flag. VERA's zero
    // source sentinel and Cell/null targets are the corresponding
    // null/non-pointer representations and therefore need no object lookup.
    for (&projectile_id, projectile) in sim.projectiles.iter() {
        if let Some(anim_id) = projectile.awaited_anim() {
            require_resolved_reference(
                anim_ids.contains(&anim_id),
                "ProjectileStore",
                projectile_id,
                "awaited_anim",
                "AnimStore",
                anim_id,
            )?;
        }
        if projectile.source_id() != crate::sim::combat::RAD_NO_ATTACKER {
            require_resolved_reference(
                entity_ids.contains(&projectile.source_id()),
                "ProjectileStore",
                projectile_id,
                "source_id",
                "EntityStore",
                projectile.source_id(),
            )?;
        }
        if let Some((target_id, target_registry, present)) = match projectile.target() {
            ProjectileTarget::Entity(id) => Some((id, "EntityStore", entity_ids.contains(&id))),
            ProjectileTarget::Terrain(id) => Some((
                id,
                "TerrainObjectStore",
                sim.terrain_objects.objects().contains_key(&id),
            )),
            ProjectileTarget::Cell { .. }
            | ProjectileTarget::DummyCell
            | ProjectileTarget::None => None,
        } {
            require_resolved_reference(
                present,
                "ProjectileStore",
                projectile_id,
                "target",
                target_registry,
                target_id,
            )?;
        }
    }

    let wave_ids = sim.waves.iter().map(|(&id, _)| id).collect::<BTreeSet<_>>();
    for (&wave_id, wave) in sim.waves.iter() {
        if let Some(owner_id) = wave.owner_id() {
            require_resolved_reference(
                entity_ids.contains(&owner_id),
                "WaveStore",
                wave_id,
                "owner_id",
                "EntityStore",
                owner_id,
            )?;
        }
        if let Some(TargetKind::Entity(target_id)) = wave.target_ref() {
            require_object_reference(
                &entity_ids,
                sim.terrain_objects.objects(),
                "WaveStore",
                wave_id,
                "target_ref",
                target_id,
            )?;
        }
    }
    for (owner_id, wave_id) in sim.active_wave_links() {
        require_resolved_reference(
            entity_ids.contains(&owner_id),
            "ActiveWaveLinks",
            owner_id,
            "owner",
            "EntityStore",
            owner_id,
        )?;
        require_resolved_reference(
            wave_ids.contains(&wave_id)
                && sim.waves.get(wave_id).and_then(|wave| wave.owner_id()) == Some(owner_id),
            "ActiveWaveLinks",
            owner_id,
            "wave",
            "WaveStore",
            wave_id,
        )?;
    }

    for object_id in sim.custom_sound_objects() {
        require_object_reference(
            &entity_ids,
            sim.terrain_objects.objects(),
            "ObjectCustomSounds",
            object_id,
            "object",
            object_id,
        )?;
    }

    for object_id in sim.substrate.pending_delete.object_tickets() {
        require_resolved_reference(
            identities.contains_key(&object_id),
            "PendingDeleteList",
            object_id,
            "entry",
            "object namespace",
            object_id,
        )?;
    }

    for factory in sim.production.factories().iter_insertion_ordered() {
        if let Some(object_id) = factory.object().and_then(|object| object.entity_id) {
            require_resolved_reference(
                entity_ids.contains(&object_id),
                "FactoryRegistry",
                u64::from(factory.owner().index()),
                "object.entity_id",
                "EntityStore",
                object_id,
            )?;
        }
    }

    // A deliberate weak identity: C4 kill credit may outlive its attacker.
    for entity in sim.substrate.entities.values_mut() {
        if entity
            .building
            .pending_c4_detonation
            .pending()
            .and_then(|pending| pending.source_entity_id())
            .is_some_and(|id| !entity_ids.contains(&id))
        {
            entity.building.pending_c4_detonation.forget_source();
        }
    }

    // The primary-factory links (the native primary byte, written by
    // Building448070/448160) name objects by id. Restore retains limbo
    // flags and prunes ids the validated object graph no longer holds.
    sim.production.retain_primary_factory_links(&entity_ids);

    Ok(())
}
