//! Snapshot tests for object references on restore: unresolved pointers are
//! refused, projectile and spawn pointers resolve atomically, terrain
//! targets keep their identity, and process dummies stay unserialized.

use super::*;
use crate::sim::snapshot::saved_ids::{SavedIdError, check_saved_interned_ids};

#[test]
fn restore_validates_references_then_rebuilds_logic_particles_and_cells() {
    use crate::rules::particle_system_type::ParticleSystemTypeId;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::particles::ParticleSystem;
    use glam::IVec3;

    let mut sim = Simulation::new();
    sim.session.set_map_name_for_test("RESTORE.MAP".to_string());
    let owner = sim.interner.intern("AMERICANS");
    let type_ref = sim.interner.intern("MTNK");

    let entity_id = sim.substrate.allocate_stable_id();
    let mut entity = GameEntity::test_default(entity_id, "MTNK", "AMERICANS", 5, 6);
    entity.set_owner_for_test(owner);
    entity.set_type_ref_for_test(type_ref);
    entity.building.pending_c4_detonation.set_for_test(Some(
        crate::sim::c4_plant::PendingC4Detonation::new(
            crate::sim::timer::CdTimer::from_raw(-1, 0),
            Some(999),
        ),
    ));
    sim.substrate.entities.insert(entity);
    sim.add_entity_occupancy(entity_id);

    let particle_id = sim.substrate.allocate_stable_id();
    sim.substrate.particle_systems.insert({
        let mut system =
            ParticleSystem::test_fixture(particle_id, ParticleSystemTypeId(0), IVec3::ZERO);
        system.set_facing_for_test(0x1d);
        system.set_target_ref_for_test(Some(crate::sim::combat::TargetKind::Entity(entity_id)));
        system.set_owner_entity_for_test(Some(entity_id));
        system
    });
    sim.substrate
        .entities
        .get_mut(entity_id)
        .unwrap()
        .seldom
        .spark_particle_system
        .set_for_test(Some(particle_id));
    sim.set_logic_order_for_test(vec![particle_id, entity_id]);

    let bytes = GameSnapshot::save_validated(&sim, 7, 8, "Restore fixture", 9);
    let mut restored = GameSnapshot::load_validated(&bytes, 7, 8, "RESTORE.MAP")
        .expect("strict snapshot")
        .sim;
    restored
        .restore_after_snapshot_load()
        .expect("valid restored object graph");

    assert_eq!(
        restored.live_object_order_snapshot(),
        vec![particle_id, entity_id]
    );
    assert!(
        restored
            .substrate
            .particle_systems
            .get(particle_id)
            .expect("particle system")
            .in_logic_vector()
    );
    assert!(
        restored
            .substrate
            .entities
            .get(entity_id)
            .expect("entity")
            .seldom
            .in_logic_vector
            .is_member()
    );
    assert!(
        restored
            .substrate
            .occupancy
            .contains_entity(5, 6, entity_id)
    );
    assert_eq!(
        restored
            .substrate
            .entities
            .get(entity_id)
            .expect("entity")
            .building
            .pending_c4_detonation
            .pending()
            .and_then(|pending| pending.source_entity_id()),
        None,
        "a successful restore cleans a dangling weak reference"
    );
    let system = restored
        .substrate
        .particle_systems
        .get(particle_id)
        .expect("particle system");
    assert_eq!(system.owner_entity(), Some(entity_id));
    assert_eq!(
        system.target_ref(),
        Some(crate::sim::combat::TargetKind::Entity(entity_id))
    );
    assert_eq!(
        restored
            .substrate
            .entities
            .get(entity_id)
            .unwrap()
            .seldom
            .spark_particle_system
            .system(),
        Some(particle_id)
    );
}

#[test]
fn restore_rejects_unresolved_native_pointer_before_weak_cleanup() {
    use crate::rules::particle_system_type::ParticleSystemTypeId;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::particles::ParticleSystem;
    use glam::IVec3;

    let mut sim = Simulation::new();
    let entity_id = sim.substrate.allocate_stable_id();
    let mut entity = GameEntity::test_default(entity_id, "MTNK", "AMERICANS", 5, 6);
    entity.building.pending_c4_detonation.set_for_test(Some(
        crate::sim::c4_plant::PendingC4Detonation::new(
            crate::sim::timer::CdTimer::from_raw(-1, 0),
            Some(999),
        ),
    ));
    sim.substrate.entities.insert(entity);

    let particle_id = sim.substrate.allocate_stable_id();
    sim.substrate.particle_systems.insert({
        let mut system =
            ParticleSystem::test_fixture(particle_id, ParticleSystemTypeId(0), IVec3::ZERO);
        system.set_facing_for_test(0x1d);
        system.set_target_ref_for_test(Some(crate::sim::combat::TargetKind::Entity(999)));
        system.set_owner_entity_for_test(Some(entity_id));
        system
    });

    assert_eq!(
        sim.restore_after_snapshot_load(),
        Err(SnapshotRestoreError::UnresolvedObjectReference {
            source_registry: "ParticleSystemStore",
            source_id: particle_id,
            field: "target_ref",
            target_registry: "EntityStore or TerrainObjectStore",
            target_id: 999,
        })
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(entity_id)
            .expect("entity")
            .building
            .pending_c4_detonation
            .pending()
            .and_then(|pending| pending.source_entity_id()),
        Some(999),
        "failed restoration must not clean a later weak reference"
    );
    assert_eq!(
        sim.substrate
            .particle_systems
            .get(particle_id)
            .expect("particle system")
            .target_ref(),
        Some(crate::sim::combat::TargetKind::Entity(999)),
        "failed restoration must not sanitize the unresolved strong reference"
    );
}

#[derive(Debug, Clone, Copy)]
struct Gsi1703Ids {
    parent: u64,
    child: u64,
    target: u64,
    projectile: u64,
}

fn gsi_17_03_projectile(
    source_id: u64,
    target: crate::sim::projectile::ProjectileTarget,
) -> crate::sim::projectile::ProjectileSpawn {
    use crate::sim::intern::InternedId;
    use crate::sim::projectile::{
        ProjectileCollisionPolicy, ProjectileCoord, ProjectilePayload, ProjectileSpawn,
        ProjectileTrajectory, ProjectileVelocity, ProjectileVisualState, TargetExpiryPolicy,
    };

    ProjectileSpawn {
        native_unique_id: 0,
        line_trail: None,
        flat: false,
        source_id,
        origin: ProjectileCoord::new(0, 0, 0),
        target,
        initial_target_position: ProjectileCoord::new(256, 256, 0),
        payload: ProjectilePayload::new(1, InternedId::from_index(0), None, false),
        speed_leptons_per_frame: 64,
        velocity: ProjectileVelocity::new(64, 0, 0),
        trajectory: ProjectileTrajectory::Straight,
        guidance: None,
        visual: ProjectileVisualState::new(0, 0, 0),
        arm_frames: 0,
        fuse_frames: None,
        ranged_fuse: false,
        tracks_target: false,
        target_expiry: TargetExpiryPolicy::Expire,
        collision: ProjectileCollisionPolicy::NONE,
    }
}

fn terrain_target_snapshot_fixture() -> (Simulation, crate::rules::ruleset::RuleSet, u64, u64) {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::combat::RAD_NO_ATTACKER;
    use crate::sim::projectile::ProjectileTarget;
    use crate::sim::terrain_object::{
        TerrainObjectState, mark_terrain_occupation, mark_terrain_raw_occupation,
    };

    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=DUMMY\n[DUMMY]\nPrimary=GUN\n[GUN]\nProjectile=SHOT\nWarhead=WH\n[SHOT]\nInviso=yes\n[WH]\nWood=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n[TerrainTypes]\n0=TREE01\n[TREE01]\nStrength=10\nArmor=wood\nTemperateOccupationBits=7\n",
    ))
    .unwrap();
    let mut sim = Simulation::new();
    sim.session.set_map_width_for_test(4);
    sim.session.set_map_height_for_test(4);
    let mut terrain = flat_terrain(4, 4);
    terrain.cell_mut_for_test(1, 1).unwrap().level = 2;
    let tree_id = sim.substrate.allocate_stable_id();
    let tree = TerrainObjectState::new(
        tree_id,
        sim.interner.intern("TREE01"),
        (1, 1),
        rules
            .terrain_object_type_case_insensitive("TREE01")
            .unwrap(),
        false,
        Some(&terrain),
    );
    let coordinate = tree.world_coord();
    mark_terrain_occupation(&tree, Some(&mut terrain));
    mark_terrain_raw_occupation(
        &mut sim.substrate.raw_cell_occupation,
        tree.cell(),
        tree.occupation_bits(),
    );
    sim.terrain_objects
        .cell_index_mut_for_test()
        .insert(tree.cell(), tree_id);
    sim.terrain_objects
        .objects_mut_for_test()
        .insert(tree_id, tree);
    *sim.resolved_terrain = Some(terrain);
    assert!(sim.register_terrain_object(tree_id, &rules));

    let projectile_id = sim.substrate.allocate_stable_id();
    let mut spawn = gsi_17_03_projectile(RAD_NO_ATTACKER, ProjectileTarget::Terrain(tree_id));
    spawn.initial_target_position = coordinate;
    spawn.payload = crate::sim::projectile::ProjectilePayload::new(
        1,
        sim.interner.intern("WH"),
        rules
            .weapon("GUN")
            .unwrap()
            .projectile
            .as_deref()
            .map(|name| sim.interner.intern(name)),
        rules.weapon("GUN").unwrap().bright,
    )
    .with_installed_weapon(Some(sim.interner.intern("GUN")));
    sim.admit_projectile(projectile_id, spawn);
    (sim, rules, tree_id, projectile_id)
}

#[test]
fn bullet_constructor_fields_roundtrip_and_hash_independently() {
    use crate::sim::projectile::ProjectilePayload;
    let (mut sim, _, _, projectile_id) = terrain_target_snapshot_fixture();
    let base = sim.projectiles.get(projectile_id).unwrap().payload();
    let baseline_hash = sim.state_hash();
    let baseline_bytes = GameSnapshot::save(&sim, 0, 0, "bullet-constructor-state", 0);
    let mut restored_baseline = GameSnapshot::load(&baseline_bytes).unwrap().sim;
    restored_baseline.restore_after_snapshot_load().unwrap();
    let restored_baseline_hash = restored_baseline.state_hash();
    let mut hashes = BTreeSet::new();
    let mut restored_hashes = BTreeSet::new();
    // Native Construct carries type+AC and Bright+E0 independently of the
    // optional WeaponType+130. In particular a null weapon must not erase
    // the type/Bright supplied to a shrapnel or transient drop Bullet.
    for kind in [None, base.projectile_type()] {
        for installed in [None, base.installed_weapon()] {
            for bright in [false, true] {
                let payload = ProjectilePayload::new(base.base_damage, base.warhead, kind, bright)
                    .with_installed_weapon(installed);
                sim.projectiles
                    .get_mut(projectile_id)
                    .unwrap()
                    .set_payload_for_test(payload);
                assert!(hashes.insert(sim.state_hash()));
                let bytes = GameSnapshot::save(&sim, 0, 0, "bullet-constructor-state", 0);
                let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
                restored.restore_after_snapshot_load().unwrap();
                assert_eq!(
                    restored.projectiles.get(projectile_id).unwrap().payload(),
                    payload
                );
                let restored_hash = restored.state_hash();
                assert!(restored_hashes.insert(restored_hash));
                assert_eq!(restored_hash == restored_baseline_hash, payload == base);
                sim.projectiles
                    .get_mut(projectile_id)
                    .unwrap()
                    .set_payload_for_test(base);
                assert_eq!(sim.state_hash(), baseline_hash);
            }
        }
    }
    assert_eq!(hashes.len(), 8);
    assert_eq!(restored_hashes.len(), 8);
}

#[test]
fn terrain_projectile_target_roundtrip_preserves_identity_coordinate_and_hash_kind() {
    use crate::sim::projectile::ProjectileTarget;
    let (mut sim, _, tree_id, projectile_id) = terrain_target_snapshot_fixture();
    let coordinate = sim.terrain_objects.objects()[&tree_id].world_coord();
    let terrain_hash = sim.state_hash();
    sim.projectiles
        .get_mut(projectile_id)
        .unwrap()
        .set_target_for_test(ProjectileTarget::Entity(tree_id));
    assert_ne!(
        terrain_hash,
        sim.state_hash(),
        "pointer kinds cannot hash alike"
    );
    sim.projectiles
        .get_mut(projectile_id)
        .unwrap()
        .set_target_for_test(ProjectileTarget::Terrain(tree_id));

    let bytes = GameSnapshot::save(&sim, 0, 0, "terrain-target", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(
        restored.projectiles.get(projectile_id).unwrap().target(),
        ProjectileTarget::Terrain(tree_id)
    );
    assert_eq!(
        restored.terrain_objects.objects()[&tree_id].world_coord(),
        coordinate
    );
    assert_eq!(
        restored
            .projectiles
            .get(projectile_id)
            .unwrap()
            .last_target_position(),
        coordinate
    );
    assert_eq!(
        restored.terrain_objects.cell_index().get(&(1, 1)),
        Some(&tree_id)
    );
}

#[test]
fn terrain_projectile_target_restore_rejects_missing_and_wrong_kind_references() {
    use crate::sim::game_entity::GameEntity;
    use crate::sim::projectile::ProjectileTarget;
    for wrong_kind in [false, true] {
        let (mut sim, _, _, projectile_id) = terrain_target_snapshot_fixture();
        let target_id = if wrong_kind {
            let id = sim.substrate.allocate_stable_id();
            sim.substrate
                .entities
                .insert(GameEntity::test_default(id, "DUMMY", "Americans", 2, 2));
            sim.register_live_object(id);
            id
        } else {
            9_999
        };
        sim.projectiles
            .get_mut(projectile_id)
            .unwrap()
            .set_target_for_test(ProjectileTarget::Terrain(target_id));
        let bytes = GameSnapshot::save(&sim, 0, 0, "bad-terrain-target", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        assert_eq!(
            restored.restore_after_snapshot_load(),
            Err(SnapshotRestoreError::UnresolvedObjectReference {
                source_registry: "ProjectileStore",
                source_id: projectile_id,
                field: "target",
                target_registry: "TerrainObjectStore",
                target_id,
            }),
            "wrong kind={wrong_kind}"
        );
        assert_eq!(
            restored.projectiles.get(projectile_id).unwrap().target(),
            ProjectileTarget::Terrain(target_id)
        );
    }
}

#[test]
fn navcom_restore_admits_only_techno_and_terrain_objects() {
    use crate::sim::components::NavTargetRef;
    use crate::sim::game_entity::GameEntity;
    // `nav_target_coordinate` resolves a NavCom Object in the entity or
    // Terrain store. Another registered identity (a projectile) or a
    // missing one names no NavCom target.
    for case in ["techno", "terrain", "projectile", "missing"] {
        let (mut sim, _, tree_id, projectile_id) = terrain_target_snapshot_fixture();
        let spawn = |sim: &mut Simulation| {
            let id = sim.substrate.allocate_stable_id();
            sim.substrate
                .entities
                .insert(GameEntity::test_default(id, "DUMMY", "Americans", 2, 2));
            sim.register_live_object(id);
            id
        };
        let foot = spawn(&mut sim);
        let target = match case {
            "techno" => spawn(&mut sim),
            "terrain" => tree_id,
            "projectile" => projectile_id,
            _ => 9_999,
        };
        sim.substrate
            .entities
            .get_mut(foot)
            .unwrap()
            .navigation
            .set_nav_com_for_test(Some(NavTargetRef::object(target)));
        let bytes = GameSnapshot::save(&sim, 0, 0, "navcom-object", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        let expected = if matches!(case, "techno" | "terrain") {
            Ok(())
        } else {
            Err(SnapshotRestoreError::UnresolvedObjectReference {
                source_registry: "EntityStore",
                source_id: foot,
                field: "navigation.nav_com",
                target_registry: "EntityStore or TerrainObjectStore",
                target_id: target,
            })
        };
        assert_eq!(restored.restore_after_snapshot_load(), expected, "{case}");
    }
}

#[test]
fn terrain_projectile_target_cell_repair_survives_pending_delete_roundtrip() {
    use crate::sim::combat::RAD_NO_ATTACKER;
    use crate::sim::projectile::ProjectileTarget;
    let (mut sim, rules, tree_id, projectile_id) = terrain_target_snapshot_fixture();
    let coordinate = sim.terrain_objects.objects()[&tree_id].world_coord();
    let warhead_id = sim.interner.intern("WH");
    crate::sim::combat::world_receiver::apply_area_damage(
        &mut sim,
        &rules,
        None,
        coordinate,
        10,
        rules.warhead("WH").unwrap(),
        true,
        (RAD_NO_ATTACKER, None, warhead_id),
        &mut crate::sim::world::FrameEffects::default(),
    );
    let cell_target = ProjectileTarget::Cell { rx: 1, ry: 1 };
    assert_eq!(
        sim.projectiles.get(projectile_id).unwrap().target(),
        cell_target
    );
    assert!(sim.substrate.pending_delete.contains_object(&tree_id));
    assert!(!sim.terrain_objects.cell_index().contains_key(&(1, 1)));
    let bytes = GameSnapshot::save(&sim, 0, 0, "retired-terrain-target", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(
        restored.projectiles.get(projectile_id).unwrap().target(),
        cell_target
    );
    assert!(restored.substrate.pending_delete.contains_object(&tree_id));
    assert!(!restored.terrain_objects.cell_index().contains_key(&(1, 1)));
    restored.flush_pending_delete();
    assert!(!restored.terrain_objects.objects().contains_key(&tree_id));
    assert_eq!(
        restored.projectiles.get(projectile_id).unwrap().target(),
        cell_target
    );
    let bytes = GameSnapshot::save(&restored, 0, 0, "deleted-terrain-target", 0);
    GameSnapshot::load(&bytes)
        .unwrap()
        .sim
        .restore_after_snapshot_load()
        .unwrap();
}

fn gsi_17_03_reference_fixture() -> (Simulation, Gsi1703Ids) {
    use crate::sim::combat::TargetKind;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::projectile::ProjectileTarget;
    use crate::sim::spawn_manager::{
        SpawnManagerMode, SpawnManagerState, SpawnSlot, SpawnSlotState,
    };
    use crate::sim::timer::CdTimer;

    let mut sim = Simulation::new();
    let parent = sim.substrate.allocate_stable_id();
    let child = sim.substrate.allocate_stable_id();
    let target = sim.substrate.allocate_stable_id();

    let mut parent_entity = GameEntity::test_default(parent, "CARRIER", "AMERICANS", 1, 1);
    parent_entity
        .building
        .pending_c4_detonation
        .set_for_test(Some(crate::sim::c4_plant::PendingC4Detonation::new(
            crate::sim::timer::CdTimer::from_raw(-1, 0),
            Some(9_999),
        )));
    let mut manager = SpawnManagerState::for_test(
        sim.interner.intern("HORNET"),
        vec![
            SpawnSlot::for_test(
                Some(child),
                SpawnSlotState::ReadyDocked,
                CdTimer::default(),
                false,
            ),
            // A second saved pointer to the same child deliberately proves
            // that swizzle aliases are valid, not duplicate ownership.
            SpawnSlot::for_test(
                Some(child),
                SpawnSlotState::ReadyDocked,
                CdTimer::default(),
                false,
            ),
        ],
    );
    manager.set_rates_for_test(45, 20);
    manager.set_current_target_for_test(Some(TargetKind::Entity(target)));
    manager.set_queued_target_for_test(Some(TargetKind::Entity(target)));
    manager.set_mode_for_test(SpawnManagerMode::Launching);
    parent_entity.spawn_manager = Some(Box::new(manager));
    sim.substrate.entities.insert(parent_entity);

    let mut child_entity = GameEntity::test_default(child, "HORNET", "AMERICANS", 2, 1);
    child_entity
        .seldom
        .spawn_owner_id
        .set_for_test(Some(parent));
    sim.substrate.entities.insert(child_entity);
    sim.substrate
        .entities
        .insert(GameEntity::test_default(target, "E1", "SOVIET", 3, 1));
    sim.register_live_object(parent);
    sim.register_live_object(child);
    sim.register_live_object(target);

    let projectile = sim.substrate.allocate_stable_id();
    sim.admit_projectile(
        projectile,
        gsi_17_03_projectile(target, ProjectileTarget::Entity(target)),
    );

    (
        sim,
        Gsi1703Ids {
            parent,
            child,
            target,
            projectile,
        },
    )
}

#[test]
fn gsi_17_03_each_spawn_and_projectile_pointer_role_must_resolve_atomically() {
    #[derive(Debug, Clone, Copy)]
    enum MissingRole {
        SpawnSlot,
        CurrentTarget,
        QueuedTarget,
        SpawnOwner,
        ProjectileSource,
        ProjectileTarget,
    }

    let cases = [
        MissingRole::SpawnSlot,
        MissingRole::CurrentTarget,
        MissingRole::QueuedTarget,
        MissingRole::SpawnOwner,
        MissingRole::ProjectileSource,
        MissingRole::ProjectileTarget,
    ];
    for role in cases {
        let (mut sim, ids) = gsi_17_03_reference_fixture();
        let (source_registry, source_id, field) = match role {
            MissingRole::SpawnSlot => {
                sim.substrate
                    .entities
                    .get_mut(ids.parent)
                    .unwrap()
                    .spawn_manager
                    .as_mut()
                    .unwrap()
                    .slots_mut_for_test()[0]
                    .set_spawn_for_test(Some(9_999));
                ("EntityStore", ids.parent, "spawn_manager.slots.spawn")
            }
            MissingRole::CurrentTarget => {
                sim.substrate
                    .entities
                    .get_mut(ids.parent)
                    .unwrap()
                    .spawn_manager
                    .as_mut()
                    .unwrap()
                    .set_current_target_for_test(Some(crate::sim::combat::TargetKind::Entity(
                        9_999,
                    )));
                ("EntityStore", ids.parent, "spawn_manager.current_target")
            }
            MissingRole::QueuedTarget => {
                sim.substrate
                    .entities
                    .get_mut(ids.parent)
                    .unwrap()
                    .spawn_manager
                    .as_mut()
                    .unwrap()
                    .set_queued_target_for_test(Some(crate::sim::combat::TargetKind::Entity(
                        9_999,
                    )));
                ("EntityStore", ids.parent, "spawn_manager.queued_target")
            }
            MissingRole::SpawnOwner => {
                sim.substrate
                    .entities
                    .get_mut(ids.child)
                    .unwrap()
                    .seldom
                    .spawn_owner_id
                    .set_for_test(Some(9_999));
                ("EntityStore", ids.child, "spawn_owner_id")
            }
            MissingRole::ProjectileSource => {
                sim.projectiles
                    .get_mut(ids.projectile)
                    .unwrap()
                    .set_source_id_for_test(9_999);
                ("ProjectileStore", ids.projectile, "source_id")
            }
            MissingRole::ProjectileTarget => {
                sim.projectiles
                    .get_mut(ids.projectile)
                    .unwrap()
                    .set_target_for_test(crate::sim::projectile::ProjectileTarget::Entity(9_999));
                ("ProjectileStore", ids.projectile, "target")
            }
        };

        assert_eq!(
            sim.restore_after_snapshot_load(),
            Err(SnapshotRestoreError::UnresolvedObjectReference {
                source_registry,
                source_id,
                field,
                target_registry: if matches!(
                    role,
                    MissingRole::CurrentTarget | MissingRole::QueuedTarget
                ) {
                    "EntityStore or TerrainObjectStore"
                } else {
                    "EntityStore"
                },
                target_id: 9_999,
            }),
            "missing {role:?} must reject the snapshot"
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(ids.parent)
                .unwrap()
                .building
                .pending_c4_detonation
                .pending()
                .and_then(|pending| pending.source_entity_id()),
            Some(9_999),
            "{role:?} rejection must precede later weak-reference cleanup"
        );
    }
}

#[test]
fn gsi_17_03_null_and_cell_reference_forms_need_no_object_identity() {
    use crate::sim::combat::{RAD_NO_ATTACKER, TargetKind};
    use crate::sim::projectile::ProjectileTarget;

    let (mut sim, ids) = gsi_17_03_reference_fixture();
    let manager = sim
        .substrate
        .entities
        .get_mut(ids.parent)
        .unwrap()
        .spawn_manager
        .as_mut()
        .unwrap();
    for slot in manager.slots_mut_for_test() {
        slot.set_spawn_for_test(None);
    }
    manager.set_current_target_for_test(Some(TargetKind::Cell(7, 8)));
    manager.set_queued_target_for_test(Some(TargetKind::Cell(9, 10)));
    sim.substrate
        .entities
        .get_mut(ids.child)
        .unwrap()
        .seldom
        .spawn_owner_id
        .set_for_test(None);
    let projectile = sim.projectiles.get_mut(ids.projectile).unwrap();
    projectile.set_source_id_for_test(RAD_NO_ATTACKER);
    projectile.set_target_for_test(ProjectileTarget::Cell { rx: 11, ry: 12 });

    let null_target_projectile = sim.substrate.allocate_stable_id();
    sim.admit_projectile(
        null_target_projectile,
        gsi_17_03_projectile(RAD_NO_ATTACKER, ProjectileTarget::None),
    );

    assert_eq!(sim.restore_after_snapshot_load(), Ok(()));
}

#[test]
fn gsi_04_01_snapshot_handoff_retains_live_process_dummy_without_serializing_it() {
    use crate::sim::combat::RAD_NO_ATTACKER;
    use crate::sim::projectile::ProjectileTarget;
    use crate::sim::{movement::locomotor::MovementLayer, occupancy::RawCellKey};

    let mut live = Simulation::new();
    let process_dummy = live.shared_cell_dummy.clone();
    process_dummy.set_level_slope(-7, 11);
    process_dummy.stamp_coord(7, 9);

    let before_neighbor_count = live.state_hash();
    process_dummy.adjust_neighbor_count(true);
    assert_ne!(before_neighbor_count, live.state_hash());

    let owner = live.intern("DummyOccupationOwner");
    let hash_before_raw = live.state_hash();
    live.substrate.raw_cell_occupation.write_occupant(
        RawCellKey::Dummy,
        MovementLayer::Ground,
        4,
        Some(owner),
        true,
    );
    let ground_hash = live.state_hash();
    assert_ne!(hash_before_raw, ground_hash);
    live.substrate.raw_cell_occupation.write_occupant(
        RawCellKey::Dummy,
        MovementLayer::Bridge,
        8,
        Some(owner),
        true,
    );
    assert_ne!(
        ground_hash,
        live.state_hash(),
        "the fallback cell's two raw planes are future-affecting"
    );
    let projectile_id = live.substrate.allocate_stable_id();
    live.admit_projectile(
        projectile_id,
        gsi_17_03_projectile(RAD_NO_ATTACKER, ProjectileTarget::DummyCell),
    );

    let hash_at_a = live.state_hash();
    process_dummy.stamp_coord(8, 10);
    assert_ne!(
        hash_at_a,
        live.state_hash(),
        "a retained Bullet pointer makes the live dummy snapshot future behavior"
    );
    process_dummy.stamp_coord(7, 9);

    let bytes = GameSnapshot::save(&live, 0, 0, "shared-dummy.map", 0);
    let cold = GameSnapshot::load(&bytes).expect("current snapshot").sim;
    assert!(
        cold.substrate
            .raw_cell_occupation
            .dummy_for_hash()
            .is_none(),
        "process-global occupation is not saved Scenario payload"
    );
    assert_eq!(
        cold.projectiles.get(projectile_id).unwrap().target(),
        ProjectileTarget::DummyCell
    );
    assert_eq!(
        cold.shared_cell_dummy.snapshot().coord,
        (0, 0),
        "the process-global CellClass bytes are not Scenario payload"
    );
    assert!(!cold.shared_cell_dummy.same_identity(&process_dummy));
    assert_eq!(cold.shared_cell_dummy.neighbor_count(), 0);

    let mut restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
    restored.retain_process_state_for_load(crate::sim::world::SnapshotProcessState::Live(&live));
    assert_eq!(
        restored.substrate.raw_cell_occupation.dummy_for_hash(),
        live.substrate.raw_cell_occupation.dummy_for_hash()
    );
    assert!(restored.shared_cell_dummy.same_identity(&process_dummy));
    assert_eq!(restored.shared_cell_dummy.neighbor_count(), 1);
    assert_eq!(
        restored.shared_cell_dummy.snapshot(),
        crate::map::resolved_terrain::SharedCellDummySnapshot {
            coord: (7, 9),
            level: -7,
            slope_type: 11,
            bridge_flags_0x1180: 0,
        },
        "fallible candidate preparation retains identity without mutating the live process"
    );
    restored
        .restore_after_snapshot_load()
        .expect("dummy CellClass target needs no Object identity fixup");

    let terrain_template = flat_terrain(2, 2);
    restored.rebuild_caches_after_load(
        terrain_template,
        crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
        &crate::sim::runtime::SimResources::empty().rules,
    );
    let rebuilt_dummy = restored
        .resolved_terrain
        .as_ref()
        .expect("rebuilt terrain")
        .shared_cell_dummy();
    assert!(rebuilt_dummy.same_identity(&process_dummy));
    assert_eq!(rebuilt_dummy.snapshot().coord, (7, 9));

    restored.reconstruct_cellclass_dummy_for_map_resize();
    assert!(
        restored
            .substrate
            .raw_cell_occupation
            .dummy_for_hash()
            .is_none(),
        "Cell47BBF0 reconstructs both raw bytes and owners"
    );
    assert_eq!(
        rebuilt_dummy.snapshot(),
        crate::map::resolved_terrain::SharedCellDummySnapshot {
            coord: (0, 0),
            level: 0,
            slope_type: 0,
            bridge_flags_0x1180: 0,
        }
    );

    process_dummy.stamp_coord(-2, 7);
    assert_eq!(
        rebuilt_dummy.snapshot().coord,
        (-2, 7),
        "the restored DummyCell target retains the same live identity after reconstruction"
    );
}

#[test]
fn gsi_04_01_successful_load_restores_real_flags_and_zeroes_only_dummy() {
    use crate::map::bridge_facts::{
        BRIDGE_FLAG_STRUCTURAL, BridgeFlagStamp, BridgeStampSlot,
        MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
    };
    use crate::rules::ini_parser::IniFile;
    use crate::rules::overlay_types::OverlayTypeRegistry;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::overlay_grid::OverlayGrid;

    let mut live = Simulation::new();
    let mut map_load_terrain = flat_terrain(4, 3);
    map_load_terrain.test_set_native_allocated_cells(&[(0, 0), (1, 1)]);
    {
        let pristine_bridge_cell = map_load_terrain.cell_mut_for_test(1, 1).unwrap();
        pristine_bridge_cell.level = 2;
        pristine_bridge_cell.slope_type = 1;
        pristine_bridge_cell.bridge_facts.raw_flags = MODELED_CELLCLASS_BRIDGE_FLAG_MASK;
    }
    let pristine_load_template = map_load_terrain.clone();
    let expected_ground_z = {
        let target = crate::sim::projectile::cell_target_coord(Some(&pristine_load_template), 1, 1);
        let cell = pristine_load_template.cell(1, 1).unwrap();
        crate::util::lepton::ground_height_leptons(cell.level, cell.slope_type, target.x, target.y)
            .expect("fixture uses a native-supported CellClass slope")
    };
    live.install_resolved_terrain_for_new_map(map_load_terrain);
    *live.overlay_grid = Some(OverlayGrid::new(4, 3));
    assert_ne!(
        live.resolved_terrain
            .as_ref()
            .unwrap()
            .cell(1, 1)
            .unwrap()
            .bridge_facts
            .raw_flags
            & BRIDGE_FLAG_STRUCTURAL,
        0,
        "the pristine allocated CellClass starts on the high bridge target surface"
    );
    let hash_with_pristine_bridge = live.state_hash();

    // The real anchor is allocated. Every neighbor is unallocated or
    // missing, so this live clear updates both serialized real authority
    // and the process dummy through their distinct native targets before
    // the snapshot is written.
    live.apply_runtime_bridge_flag_stamp(BridgeFlagStamp::new((1, 1), 0, false));
    let process_dummy = live.effective_shared_cell_dummy();
    assert_eq!(
        live.resolved_terrain
            .as_ref()
            .unwrap()
            .cell(1, 1)
            .unwrap()
            .bridge_facts
            .raw_flags
            & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
        0,
        "the live runtime setter clears the allocated CellClass before save"
    );
    assert_eq!(
        *live.real_cell_bridge_flags_0x1180,
        live.resolved_terrain
            .as_ref()
            .unwrap()
            .capture_real_cell_bridge_flags_0x1180(),
        "serialized real-cell authority records the runtime-cleared value"
    );
    process_dummy.reconstruct_for_map_resize();
    let hash_after_runtime_clear = live.state_hash();
    assert_ne!(
        hash_after_runtime_clear, hash_with_pristine_bridge,
        "the future-affecting real-cell value authority is hashed even with a clear dummy"
    );
    assert!(live.rebuild_dynamic_navigation(&crate::sim::runtime::SimResources::empty().rules));
    process_dummy.stamp_coord(-23, 17);
    process_dummy.set_level_slope(-7, 11);
    process_dummy.apply_bridge_flag_slot(BridgeStampSlot::Anchor, true);
    let dirty_dummy_before_load = process_dummy.snapshot();
    assert_ne!(dirty_dummy_before_load.bridge_flags_0x1180, 0);

    // In-scenario process state retains the shared Dummy, while native
    // Scenario deserialization independently resets its RNG to Seed(0).
    *live.scenario_rng = crate::sim::rng::SimRng::new(0);
    let bytes = GameSnapshot::save(&live, 0, 0, "bridge-dummy.map", 0);
    let mut restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
    restored.retain_process_state_for_load(crate::sim::world::SnapshotProcessState::Live(&live));
    restored
        .restore_after_snapshot_load()
        .expect("snapshot references restore");
    restored.rebuild_caches_after_load(
        pristine_load_template,
        crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
        &crate::sim::runtime::SimResources::empty().rules,
    );
    let rebuilt_terrain = restored.resolved_terrain.as_ref().unwrap();
    assert_eq!(
        rebuilt_terrain.cell(1, 1).unwrap().bridge_facts.raw_flags
            & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
        MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
        "candidate cache rebuild starts from the nonzero pristine map template"
    );
    assert_eq!(
        crate::sim::projectile::cell_target_coord(Some(rebuilt_terrain), 1, 1).z,
        expected_ground_z + crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS,
        "before direct value restore the pristine raw 0x100 selects the +416 target surface"
    );
    let pristine_candidate_authority = rebuilt_terrain.capture_real_cell_bridge_flags_0x1180();
    let serialized_cleared_authority = (*restored.real_cell_bridge_flags_0x1180).clone();
    assert_ne!(serialized_cleared_authority, pristine_candidate_authority);
    let hash_with_serialized_clear = restored.state_hash();
    *restored.real_cell_bridge_flags_0x1180 = pristine_candidate_authority.clone();
    let hash_if_pristine_template_were_authority = restored.state_hash();
    assert_ne!(
        hash_with_serialized_clear, hash_if_pristine_template_were_authority,
        "the canonical hash distinguishes the saved clear from the pristine nonzero template"
    );
    *restored.real_cell_bridge_flags_0x1180 = serialized_cleared_authority.clone();
    assert_eq!(restored.state_hash(), hash_with_serialized_clear);
    let candidate_dummy_before_authority_restore =
        restored.effective_shared_cell_dummy().snapshot();
    let ini = IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n[OverlayTypes]\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("empty map-authority rules");
    restored
        .restore_map_authority_after_snapshot_load(&rules, &OverlayTypeRegistry::empty(), None)
        .expect("serialized real CellClass values restore directly");

    let candidate_dummy = restored.effective_shared_cell_dummy();
    assert!(candidate_dummy.same_identity(&process_dummy));
    assert_eq!(
        candidate_dummy.snapshot(),
        candidate_dummy_before_authority_restore,
        "direct real-cell restoration must not lookup, stamp, or mutate the dummy"
    );
    assert_eq!(candidate_dummy.snapshot(), dirty_dummy_before_load);
    let restored_terrain = restored.resolved_terrain.as_ref().unwrap();
    assert_eq!(
        restored_terrain.cell(1, 1).unwrap().bridge_facts.raw_flags & BRIDGE_FLAG_STRUCTURAL,
        0,
        "the saved clear removes the pristine real CellClass structural bit"
    );
    assert_eq!(
        restored_terrain.cell(1, 1).unwrap().bridge_facts.raw_flags
            & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
        0,
        "direct CellClass value restore replaces the complete pristine modeled mask"
    );
    assert_eq!(
        crate::sim::projectile::cell_target_coord(Some(restored_terrain), 1, 1).z,
        expected_ground_z,
        "restored raw 0x100 clear removes +416 while retaining the 104-lepton ground kernel"
    );
    let saved_authority = save_codec::serialize(&live).unwrap();
    let loaded_authority = save_codec::serialize(&restored).unwrap();
    assert!(
        saved_authority == loaded_authority,
        "real Cell authority differs at byte {:?}; lengths {} vs {}",
        saved_authority
            .iter()
            .zip(&loaded_authority)
            .position(|(a, b)| a != b),
        saved_authority.len(),
        loaded_authority.len()
    );
    assert_zone_grids_equivalent(
        live.zone_grid.as_ref().unwrap(),
        restored.zone_grid.as_ref().unwrap(),
    );
    assert_eq!(
        restored.state_hash(),
        live.state_hash(),
        "completed native hierarchy rebuild matches the published saved-clear world"
    );
    assert_eq!(
        *restored.real_cell_bridge_flags_0x1180,
        serialized_cleared_authority
    );
    assert_eq!(
        restored_terrain.cell(0, 0).unwrap().bridge_facts.raw_flags
            & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
        0,
        "untouched allocated cells retain their exact saved zero"
    );
    assert!(restored_terrain.cell(1, 0).is_none());
    assert_eq!(
        restored_terrain.cells()[1].bridge_facts.raw_flags & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
        0,
        "unallocated storage is not promoted into real-cell authority"
    );

    // MouseClass::Load reaches Resize only at accepted commit. That ctor
    // zeros the same dummy identity and does not rewrite loaded real cells.
    restored.reconstruct_cellclass_dummy_for_map_resize();

    let reconstructed = restored.effective_shared_cell_dummy();
    assert!(reconstructed.same_identity(&process_dummy));
    assert_eq!(
        reconstructed.snapshot(),
        crate::map::resolved_terrain::SharedCellDummySnapshot {
            coord: (0, 0),
            level: 0,
            slope_type: 0,
            bridge_flags_0x1180: 0,
        }
    );
    assert_eq!(
        restored
            .resolved_terrain
            .as_ref()
            .unwrap()
            .cell(1, 1)
            .unwrap()
            .bridge_facts
            .raw_flags
            & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
        0,
        "dummy reconstruction must not resurrect pristine real CellClass flags"
    );
    assert_eq!(
        crate::sim::projectile::cell_target_coord(restored.resolved_terrain.as_ref(), 1, 1,).z,
        expected_ground_z,
        "accepted Resize keeps the restored real CellClass on the 104-lepton ground surface"
    );
    assert_eq!(
        *restored.real_cell_bridge_flags_0x1180, serialized_cleared_authority,
        "accepted Resize clears only the dummy and retains saved real-cell hash authority"
    );
}

#[test]
fn gsi_17_03_repeated_aliases_do_not_imply_reciprocal_spawn_ownership() {
    let (mut sim, ids) = gsi_17_03_reference_fixture();
    sim.substrate
        .entities
        .get_mut(ids.child)
        .unwrap()
        .seldom
        .spawn_owner_id
        .set_for_test(Some(ids.target));

    assert_eq!(sim.restore_after_snapshot_load(), Ok(()));
    assert_eq!(
        sim.substrate
            .entities
            .get(ids.parent)
            .unwrap()
            .spawn_manager
            .as_ref()
            .unwrap()
            .slots()
            .iter()
            .map(|slot| slot.spawn())
            .collect::<Vec<_>>(),
        vec![Some(ids.child), Some(ids.child)]
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(ids.child)
            .unwrap()
            .seldom
            .spawn_owner_id
            .owner(),
        Some(ids.target)
    );
}

#[test]
fn restore_clears_move_sound_without_playback_or_rng_draw() {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::game_entity::GameEntity;

    let rules = RuleSet::from_ini_with_sounds_for_test(
        &IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n0=TEST\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [TEST]\nMoveSound=VMove\n",
        ),
        &["VMove"],
    )
    .expect("move-sound rules");
    let mut sim = Simulation::new();
    sim.session
        .set_map_name_for_test("MOVESOUND.MAP".to_string());
    let owner = sim.interner.intern("AMERICANS");
    let type_ref = sim.interner.intern("TEST");
    sim.interner.intern("VMove");
    let entity_id = sim.substrate.allocate_stable_id();
    let mut entity = GameEntity::test_default(entity_id, "TEST", "AMERICANS", 7, 9);
    entity.set_owner_for_test(owner);
    entity.set_type_ref_for_test(type_ref);
    entity.move_sound = crate::sim::world::MoveSoundState::from_raw_for_test(true, 2);
    sim.substrate.entities.insert(entity);

    let bytes = GameSnapshot::save_validated(&sim, 17, 18, "Move sound fixture", 19);
    let mut restored = GameSnapshot::load_validated(&bytes, 17, 18, "MOVESOUND.MAP")
        .expect("strict snapshot")
        .sim;
    let rng_before = restored.rng_state();
    restored
        .restore_after_snapshot_load()
        .expect("valid restored object graph");
    restored.resolve_type_handles(&rules);
    assert_eq!(restored.rng_state(), rng_before);
    let entity = restored.substrate.entities.get(entity_id).expect("entity");
    assert!(!entity.move_sound.is_active());
    assert_eq!(entity.move_sound.countdown(), 0);
    assert!(restored.sound_events.is_empty());
}

/// A cell TarCom survives serialize → deserialize as the same
/// `TargetKind::Cell` variant.
#[test]
fn cell_attack_target_round_trips_through_snapshot() {
    use crate::sim::combat::TargetKind;
    use crate::sim::game_entity::GameEntity;

    let mut sim = Simulation::new();
    let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
    entity
        .attack_target
        .set_for_test(Some(TargetKind::Cell(50, 50)));
    sim.substrate.entities.insert(entity);

    let bytes = GameSnapshot::save(&sim, 0, 0, "test_map", 0);
    let loaded = GameSnapshot::load(&bytes).expect("load should succeed");
    let restored = loaded
        .sim
        .substrate
        .entities
        .get(1)
        .expect("entity should be restored")
        .attack_target
        .target()
        .expect("attack_target should be restored");
    assert!(matches!(restored, TargetKind::Cell(50, 50)));
}

/// An object's saved type and owner ids and the interner that names them all
/// come from the file. An id past that interner refuses the player load
/// before any restore step resolves it and panics.
#[test]
fn object_type_id_past_the_saved_interner_refuses_the_load() {
    let mut sim = Simulation::new();
    let entity = crate::sim::game_entity::GameEntity::test_default(1, "MTNK", "AMERICANS", 5, 6);
    let type_id = entity.type_ref().index();
    sim.substrate.entities.insert(entity);
    *sim.interner = crate::sim::intern::StringInterner::new();
    let bytes = GameSnapshot::save(&sim, 0, 0, "unknown-type", 0);
    let Err(error) = GameSnapshot::load(&bytes).unwrap().restore_player_world(
        crate::sim::world::SnapshotProcessState::Live(&Simulation::new()),
        crate::rules::ruleset::RuleSet::empty_for_test(),
        flat_terrain(8, 8),
        Default::default(),
        PlayerSaveCells::FixtureBaseline,
    ) else {
        panic!("a type id past the saved interner must refuse the load");
    };
    assert_eq!(
        error,
        SnapshotRestoreError::UnknownInternedId {
            registry: "object",
            object_id: 1,
            field: "type",
            id: type_id,
        },
    );
}

/// A bullet's saved warhead id is resolved when it detonates, so one past the
/// saved interner refuses the load too.
#[test]
fn bullet_warhead_id_past_the_saved_interner_refuses_the_load() {
    use crate::sim::combat::RAD_NO_ATTACKER;
    use crate::sim::projectile::{ProjectilePayload, ProjectileTarget};

    let mut sim = Simulation::new();
    let unknown = crate::sim::intern::InternedId::from_index(9_999);
    let mut spawn = gsi_17_03_projectile(RAD_NO_ATTACKER, ProjectileTarget::None);
    spawn.payload = ProjectilePayload::new(1, unknown, None, false);
    let bullet = sim.substrate.allocate_stable_id();
    sim.admit_projectile(bullet, spawn);
    let bytes = GameSnapshot::save(&sim, 0, 0, "unknown-warhead", 0);
    let Err(error) = GameSnapshot::load(&bytes).unwrap().restore_player_world(
        crate::sim::world::SnapshotProcessState::Live(&Simulation::new()),
        crate::rules::ruleset::RuleSet::empty_for_test(),
        flat_terrain(8, 8),
        Default::default(),
        PlayerSaveCells::FixtureBaseline,
    ) else {
        panic!("a warhead id past the saved interner must refuse the load");
    };
    assert_eq!(
        error,
        SnapshotRestoreError::UnknownInternedId {
            registry: "bullet",
            object_id: bullet,
            field: "warhead",
            id: unknown.index(),
        },
    );
}

/// The Team VM's saved ids (here a ScriptType's) are resolved by name when
/// Teams are made and traced, so one past the saved interner refuses the load.
/// No list names the field: the walk over the saved state finds it.
#[test]
fn script_type_id_past_the_saved_interner_refuses_the_load() {
    use crate::sim::team_script_vm::TeamScriptDefinition;

    let mut sim = Simulation::new();
    let unknown = crate::sim::intern::InternedId::from_index(9_999);
    sim.team_script_vm.register_script(TeamScriptDefinition {
        id: unknown,
        actions: Vec::new(),
        source: crate::rules::team_ai_ini::TeamAiDefinitionSource::FixedAimd,
    });
    let bytes = GameSnapshot::save(&sim, 0, 0, "unknown-script", 0);
    let Err(error) = GameSnapshot::load(&bytes).unwrap().restore_player_world(
        crate::sim::world::SnapshotProcessState::Live(&Simulation::new()),
        crate::rules::ruleset::RuleSet::empty_for_test(),
        flat_terrain(8, 8),
        Default::default(),
        PlayerSaveCells::FixtureBaseline,
    ) else {
        panic!("a ScriptType id past the saved interner must refuse the load");
    };
    assert_eq!(
        error,
        SnapshotRestoreError::UnknownSavedId(SavedIdError::Unknown {
            owner: "TeamScriptVm",
            field: "scripts",
            id: unknown.index(),
        }),
    );
}

/// The walk reads an id wherever the saved shape holds one (a map key, an
/// `Option`, an enum's field, a tuple) and takes no other `u32` for one.
#[test]
fn saved_id_walk_finds_ids_in_nested_shapes_and_ignores_plain_numbers() {
    use crate::sim::intern::InternedId;
    use std::collections::BTreeMap;

    #[derive(serde::Serialize)]
    enum Order {
        Build { what: InternedId },
    }
    #[derive(serde::Serialize)]
    struct Saved {
        count: u32,
        by_name: BTreeMap<InternedId, (u32, Option<InternedId>)>,
        orders: Vec<Order>,
    }
    let id = InternedId::from_index;
    let saved = |key, value, what| Saved {
        count: 4_000_000_000,
        by_name: BTreeMap::from([(id(key), (4_000_000_000, Some(id(value))))]),
        orders: vec![Order::Build { what: id(what) }],
    };
    let unknown = |owner, field, id| Err(SavedIdError::Unknown { owner, field, id });
    assert_eq!(check_saved_interned_ids(&saved(2, 2, 2), 3), Ok(()));
    assert_eq!(
        check_saved_interned_ids(&saved(3, 2, 2), 3),
        unknown("Saved", "by_name", 3)
    );
    assert_eq!(
        check_saved_interned_ids(&saved(2, 7, 2), 3),
        unknown("Saved", "by_name", 7)
    );
    assert_eq!(
        check_saved_interned_ids(&saved(2, 2, 9), 3),
        unknown("Order", "what", 9)
    );
}
