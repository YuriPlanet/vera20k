//! Focused tests for the shared runtime-object identity namespace.

use std::collections::BTreeSet;

use crate::map::overlay::TerrainObject;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;
use crate::sim::projectile::{
    ProjectileCollisionPolicy, ProjectileCoord, ProjectilePayload, ProjectileSpawn,
    ProjectileTarget, ProjectileTrajectory, ProjectileVelocity, ProjectileVisualState,
    TargetExpiryPolicy,
};
use crate::sim::snapshot::{GameSnapshot, SnapshotRestoreError};
use crate::sim::terrain_object::TerrainObjectState;
use crate::sim::terrain_spawn::construct_terrain_objects;
use crate::sim::wave::Wave;
use crate::sim::world::Simulation;

fn terrain(stable_id: u64) -> TerrainObjectState {
    {
        let mut terrain = TerrainObjectState::for_test(stable_id, InternedId::from_index(0), 3, 4);
        terrain.set_health_for_test(10);
        terrain.set_max_health_for_test(10);
        terrain
    }
}

fn projectile(source_id: u64) -> ProjectileSpawn {
    ProjectileSpawn {
        native_unique_id: 0,
        line_trail: None,
        flat: false,
        source_id,
        origin: ProjectileCoord::new(0, 0, 0),
        target: ProjectileTarget::Cell { rx: 2, ry: 0 },
        initial_target_position: ProjectileCoord::new(512, 0, 0),
        payload: ProjectilePayload::new(10, InternedId::from_index(0), None, false),
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

#[test]
fn gsi_05_01_runtime_objects_share_identity_and_continue_after_roundtrip() {
    let mut sim = Simulation::new();

    let entity_id = sim.substrate.allocate_stable_id();
    sim.substrate.entities.insert(GameEntity::test_default(
        entity_id,
        "MTNK",
        "Americans",
        1,
        2,
    ));

    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
         [TerrainTypes]\n0=TREE01\n[TREE01]\nStrength=10\n",
    ))
    .expect("terrain rules");
    assert_eq!(
        construct_terrain_objects(
            &mut sim,
            &[TerrainObject {
                rx: 3,
                ry: 4,
                name: "TREE01".to_string(),
            }],
            &rules,
            false,
        ),
        1
    );
    let terrain_id = *sim
        .terrain_objects
        .objects()
        .keys()
        .next()
        .expect("constructed terrain");

    let projectile_id = sim.substrate.allocate_stable_id();
    sim.admit_projectile(projectile_id, projectile(entity_id));
    let wave_id = sim.substrate.allocate_stable_id();
    sim.admit_wave(
        wave_id,
        Wave::new(
            3,
            ProjectileCoord::new(0, 0, 0),
            ProjectileCoord::new(512, 0, 0),
        ),
    );

    let ids = [entity_id, terrain_id, projectile_id, wave_id];
    assert!(ids.iter().all(|id| *id != 0));
    assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(ids.into_iter().collect::<BTreeSet<_>>().len(), ids.len());

    let next_id = sim.next_stable_object_id();
    // In-scenario retail load consumes the saved Scenario RNG bytes and then
    // resets that cursor to seed zero. Normalize the source at that proven
    // post-load baseline so this test isolates object identity preservation.
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let hash_before = sim.state_hash();
    let bytes = GameSnapshot::save(&sim, 0, 0, "identity", 0);
    let mut restored = GameSnapshot::load(&bytes).expect("identity snapshot").sim;
    restored
        .restore_after_snapshot_load()
        .expect("one global restored namespace");

    assert_eq!(restored.state_hash(), hash_before);
    assert!(restored.substrate.entities.contains(entity_id));
    assert!(restored.terrain_objects.objects().contains_key(&terrain_id));
    assert_eq!(
        restored.projectiles.get(projectile_id).unwrap().id(),
        projectile_id
    );
    assert_eq!(
        restored
            .waves
            .iter()
            .next()
            .map(|(&id, wave)| (id, wave.id())),
        Some((wave_id, wave_id))
    );
    assert_eq!(restored.next_stable_object_id(), next_id);
    assert_eq!(restored.substrate.allocate_stable_id(), next_id);
}

#[test]
fn gsi_05_01_restore_rejects_zero_ids_in_each_added_store() {
    let mut terrain_sim = Simulation::new();
    terrain_sim
        .terrain_objects
        .objects_mut_for_test()
        .insert(0, terrain(0));
    assert_eq!(
        terrain_sim.restore_after_snapshot_load(),
        Err(SnapshotRestoreError::ReservedObjectId {
            registry: "TerrainObjectStore"
        })
    );

    let mut projectile_sim = Simulation::new();
    projectile_sim.projectiles.spawn(0, projectile(0));
    assert_eq!(
        projectile_sim.restore_after_snapshot_load(),
        Err(SnapshotRestoreError::ReservedObjectId {
            registry: "ProjectileStore"
        })
    );

    let mut wave_sim = Simulation::new();
    wave_sim.waves.spawn(
        0,
        Wave::new(
            3,
            ProjectileCoord::new(0, 0, 0),
            ProjectileCoord::new(1, 0, 0),
        ),
    );
    assert_eq!(
        wave_sim.restore_after_snapshot_load(),
        Err(SnapshotRestoreError::ReservedObjectId {
            registry: "WaveStore"
        })
    );
}

#[test]
fn gsi_05_01_restore_rejects_mismatch_duplicate_and_counter_behind() {
    let mut mismatch = Simulation::new();
    let registry_id = mismatch.substrate.allocate_stable_id();
    mismatch
        .terrain_objects
        .objects_mut_for_test()
        .insert(registry_id, terrain(registry_id + 1));
    assert_eq!(
        mismatch.restore_after_snapshot_load(),
        Err(SnapshotRestoreError::ObjectIdentityMismatch {
            registry: "TerrainObjectStore",
            registry_id,
            object_id: registry_id + 1,
        })
    );

    let mut duplicate = Simulation::new();
    let duplicate_id = duplicate.substrate.allocate_stable_id();
    duplicate
        .projectiles
        .spawn(duplicate_id, projectile(duplicate_id));
    duplicate.waves.spawn(
        duplicate_id,
        Wave::new(
            3,
            ProjectileCoord::new(0, 0, 0),
            ProjectileCoord::new(1, 0, 0),
        ),
    );
    assert_eq!(
        duplicate.restore_after_snapshot_load(),
        Err(SnapshotRestoreError::DuplicateObjectIdentity {
            object_id: duplicate_id,
            first_registry: "ProjectileStore",
            second_registry: "WaveStore",
        })
    );

    let mut behind = Simulation::new();
    let highest_id = behind.substrate.allocate_stable_id();
    behind.waves.spawn(
        highest_id,
        Wave::new(
            3,
            ProjectileCoord::new(0, 0, 0),
            ProjectileCoord::new(1, 0, 0),
        ),
    );
    behind.set_next_stable_object_id_for_test(highest_id);
    assert_eq!(
        behind.restore_after_snapshot_load(),
        Err(SnapshotRestoreError::ObjectIdCounterBehind {
            next_id: highest_id,
            highest_id,
        })
    );
}
