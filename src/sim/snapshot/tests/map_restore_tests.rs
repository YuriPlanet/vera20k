//! Snapshot tests for map and navigation restore: overlay passability,
//! live terrain assets, truncated cell storage, terrain object caches and
//! the deterministic rebuild of zones and cell order after load.

use super::*;

#[test]
fn retail_static_tree_display_survives_load_without_logic_membership() {
    use crate::sim::world::display_layers::DisplayLayer;

    let Some(battle) =
        crate::rules::retail_ini_fixture::retail_battle_rules_for_map("Dustbowl.mmx")
    else {
        return;
    };
    let rules = &battle.rules;
    let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
        &battle.processed_rules,
        Some(&battle.fixed_art),
    );
    let terrain = flat_terrain(8, 8);
    let mut sim = Simulation::new();
    *sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(8, 8));
    sim.install_resolved_terrain_for_new_map(terrain.clone());
    crate::sim::terrain_spawn::construct_terrain_objects(
        &mut sim,
        &[
            crate::map::overlay::TerrainObject {
                rx: 3,
                ry: 4,
                name: "TREE01".into(),
            },
            crate::map::overlay::TerrainObject {
                rx: 5,
                ry: 4,
                name: "TIBTRE01".into(),
            },
        ],
        rules,
        false,
    );
    let tree = sim.terrain_objects.id_at((3, 4)).unwrap();
    let spawner = sim.terrain_objects.id_at((5, 4)).unwrap();
    assert_eq!(sim.live_object_order_snapshot(), [spawner]);
    assert_eq!(
        sim.substrate.display.members(DisplayLayer::GROUND),
        [tree, spawner]
    );
    let bytes = GameSnapshot::save(&sim, 0, 0, "terrain-membership", 0);
    let mut loaded = GameSnapshot::load(&bytes).unwrap().sim;
    loaded
        .restore_after_snapshot_load()
        .expect("a live static tree needs no Logic slot");
    loaded.rebuild_caches_after_load(
        terrain,
        crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
        rules,
    );
    loaded
        .restore_map_authority_after_snapshot_load(rules, &registry, None)
        .unwrap();
    assert_eq!(loaded.live_object_order_snapshot(), [spawner]);
    assert_eq!(
        loaded.substrate.display.members(DisplayLayer::GROUND),
        [tree, spawner]
    );
    assert!(!loaded.terrain_objects.get(tree).unwrap().in_logic_vector());
    let warhead_ref = loaded.interner.intern("HE");
    loaded.commit_direct_terrain_damage_receiver(
        rules,
        Some(&registry),
        crate::sim::combat::TerrainDamageEvent {
            damage_writeback: None,
            stable_id: tree,
            rx: 3,
            ry: 4,
            damage: loaded.terrain_objects.get(tree).unwrap().health(),
            distance_leptons: 0,
            warhead_ref,
            near_center_ic_isolation_eligible: false,
        },
        true,
        &mut crate::sim::world::FrameEffects::default(),
    );
    assert_eq!(
        loaded.substrate.display.members(DisplayLayer::GROUND),
        [spawner]
    );
    assert_eq!(loaded.live_object_order_snapshot(), [spawner]);
    assert!(loaded.terrain_objects.id_at((3, 4)).is_none());
    assert_eq!(loaded.substrate.pending_delete.object_ids(), [tree]);

    loaded.set_logic_order_for_test(Vec::new());
    loaded.rebuild_logic_membership();
    assert_eq!(
        loaded
            .restore_map_authority_after_snapshot_load(rules, &registry, None)
            .unwrap_err(),
        SnapshotRestoreError::MissingRequiredLogicIdentity {
            registry: "TerrainObjectStore",
            object_id: spawner,
        },
    );
    assert_eq!(
        loaded
            .restore_map_authority_after_snapshot_load(
                crate::rules::ruleset::RuleSet::empty_for_test(),
                &registry,
                None,
            )
            .unwrap_err(),
        SnapshotRestoreError::MissingTerrainType {
            object_id: spawner,
            type_name: "TIBTRE01".into(),
        },
    );
}

fn rebuild_load_caches(sim: &mut Simulation, terrain: ResolvedTerrainGrid) {
    // Synthetic fixtures use unchecked snapshots and often bypass the
    // monotonic allocators. Rebuild their substrate caches directly; the
    // production path uses `restore_after_snapshot_load`.
    sim.substrate.entities.rebuild_class_registries();
    sim.rebuild_logic_membership();
    sim.substrate
        .occupancy
        .restore_memberships(&sim.substrate.entities)
        .expect("fixture has valid saved Cell references");
    sim.rebuild_caches_after_load(
        terrain,
        crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
        &crate::sim::runtime::SimResources::empty().rules,
    );
}

#[test]
fn snapshot_restore_replays_overlay_passability_and_publishes_canonical_navigation() {
    use crate::map::overlay::OverlayEntry;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::overlay_types::OverlayTypeRegistry;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::overlay_grid::{OverlayGrid, recalc_overlay_passability};
    use crate::sim::pathfinding::zone_map::ZONE_INVALID;

    let ini = IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
         [OverlayTypes]\n0=WALL\n\
         [WALL]\nWall=yes\nStrength=100\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("snapshot overlay rules");
    let registry = OverlayTypeRegistry::from_ini(&ini, None);

    let cleared_since_map_load = (0, 0);
    let runtime_wall = (2, 0);
    let mut map_terrain = flat_terrain(3, 1);
    let mut map_overlays = OverlayGrid::from_overlay_entries(
        &[OverlayEntry {
            rx: cleared_since_map_load.0,
            ry: cleared_since_map_load.1,
            overlay_id: 0,
            frame: 0,
        }],
        3,
        1,
    );
    assert!(recalc_overlay_passability(
        &mut map_overlays,
        &mut map_terrain,
        &registry,
        cleared_since_map_load.0,
        cleared_since_map_load.1,
    ));
    assert!(
        map_terrain
            .cell(cleared_since_map_load.0, cleared_since_map_load.1)
            .expect("map wall terrain")
            .overlay_blocks
    );

    let mut sim = Simulation::new();
    let runtime_overlays = OverlayGrid::from_overlay_entries(
        &[OverlayEntry {
            rx: runtime_wall.0,
            ry: runtime_wall.1,
            overlay_id: 0,
            frame: 0x21,
        }],
        3,
        1,
    );
    // This fixture places a live wall but is not a blocker-count subject:
    // its plane stays zero. A real wall always carries its
    // `OverlayClass::Mark` increments.
    *sim.overlay_grid = Some(runtime_overlays);
    sim.install_resolved_terrain_for_new_map(map_terrain.clone());
    sim.restore_map_authority_after_snapshot_load(&rules, &registry, None)
        .expect("publish the live runtime overlay navigation before saving");
    // Original Scenario full-load resets this stream to Seed(0). Match
    // that retained load policy when comparing the published live fixture.
    *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let authoritative_hash = sim.state_hash();

    let bytes = GameSnapshot::save(&sim, 0, 0, "overlay_restore.map", 0);
    let mut restored = GameSnapshot::load(&bytes)
        .expect("snapshot with runtime overlay authority")
        .sim;
    restored
        .restore_after_snapshot_load()
        .expect("stable snapshot identity");
    assert!(
        restored
            .zone_grid
            .as_ref()
            .unwrap()
            .is_native_load_pending()
    );
    restored.rebuild_caches_after_load(
        map_terrain,
        crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
        &rules,
    );

    let restore_output = restored
        .restore_map_authority_after_snapshot_load(&rules, &registry, None)
        .expect("restored overlay and navigation authority");
    let terrain = restored
        .resolved_terrain
        .as_ref()
        .expect("restored terrain");
    assert!(!terrain.cell(0, 0).expect("cleared cell").overlay_blocks);
    assert!(
        terrain
            .cell(2, 0)
            .expect("runtime wall cell")
            .overlay_blocks
    );
    let path = restored.path_grid().expect("canonical restored path grid");
    assert!(path.is_walkable(0, 0));
    assert!(!path.is_walkable(2, 0));
    let normal_zones = restored
        .zone_grid
        .as_ref()
        .and_then(|zones| zones.map_for(MovementZone::Normal))
        .expect("canonical restored ground zones");
    assert_ne!(
        normal_zones.zone_at(0, 0, MovementLayer::Ground),
        ZONE_INVALID
    );
    assert_eq!(
        normal_zones.zone_at(2, 0, MovementLayer::Ground),
        ZONE_INVALID
    );
    assert_eq!(restore_output.occupied_overlays.len(), 1);
    assert_eq!(
        (
            restore_output.occupied_overlays[0].rx,
            restore_output.occupied_overlays[0].ry,
            restore_output.occupied_overlays[0].overlay_id,
            restore_output.occupied_overlays[0].frame,
        ),
        (2, 0, 0, 0x21)
    );
    assert_eq!(restore_output.native_tiberium_stats, Default::default());
    let saved_authority = save_codec::serialize(&sim).unwrap();
    let loaded_authority = save_codec::serialize(&restored).unwrap();
    assert!(
        saved_authority == loaded_authority,
        "overlay authority differs at byte {:?}; lengths {} vs {}",
        saved_authority
            .iter()
            .zip(&loaded_authority)
            .position(|(a, b)| a != b),
        saved_authority.len(),
        loaded_authority.len()
    );
    assert_zone_grids_equivalent(
        sim.zone_grid.as_ref().unwrap(),
        restored.zone_grid.as_ref().unwrap(),
    );
    assert_eq!(restored.state_hash(), authoritative_hash);
}

fn terrain_asset_restore_fixture() -> (
    Simulation,
    crate::rules::ruleset::RuleSet,
    crate::rules::overlay_types::OverlayTypeRegistry,
) {
    let ini = crate::rules::ini_parser::IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
         [OverlayTypes]\n0=WALL\n[WALL]\nWall=yes\nStrength=100\n",
    );
    let rules = crate::rules::ruleset::RuleSet::from_ini(&ini).unwrap();
    let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);
    let mut sim = Simulation::new();
    sim.set_playfield_bounds_for_test(Some(crate::sim::cell_rect::PlayfieldBounds {
        base: 2,
        off_fc: -8,
        off_100: -8,
        off_104: 16,
        off_108: 16,
    }));
    sim.set_playfield_size_height_for_test(Some(2));
    *sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(4, 4));
    sim.overlay_grid.as_mut().unwrap().place_overlay(2, 2, 0, 0);
    sim.install_resolved_terrain_for_new_map(flat_terrain(4, 4));
    let mut replaced = sim
        .resolved_terrain
        .as_ref()
        .unwrap()
        .cell(1, 2)
        .unwrap()
        .clone();
    replaced.final_tile_index = 71;
    sim.dynamic_terrain_cells.record(&replaced);
    sim.real_cell_bridge_flags_0x1180.set_allocated_cell(
        2 * 4 + 1,
        crate::map::bridge_facts::BRIDGE_FLAG_DIRECTION_ZERO,
    );
    (sim, rules, registry)
}

#[test]
fn snapshot_restore_prepares_live_terrain_assets_before_map_publication() {
    let (mut sim, rules, registry) = terrain_asset_restore_fixture();
    let rng_before = sim.rng_state();
    let mut expected_main = sim.main_rng.clone();
    let expected_word = expected_main.next_u32();
    let mut calls = 0;
    let mut prepare =
        |terrain: &mut ResolvedTerrainGrid, main: &mut crate::sim::rng::SimRng, size_width: i32| {
            calls += 1;
            assert_eq!(
                size_width, 2,
                "the native iterator uses Size, not grid width"
            );
            assert_eq!(terrain.cell(1, 2).unwrap().final_tile_index, 71);
            assert_eq!(terrain.cell(1, 2).unwrap().bridge_facts.raw_flags, 0);
            assert!(!terrain.cell(2, 2).unwrap().overlay_blocks);
            assert_eq!(main.next_u32(), expected_word);
            Ok(())
        };

    sim.restore_map_authority_after_snapshot_load(&rules, &registry, Some(&mut prepare))
        .expect("prepare assets from dynamic cells before final map publication");

    assert_eq!(calls, 1);
    assert_eq!(sim.main_rng.logical_state(), expected_main.logical_state());
    assert_eq!(sim.rng_state().scenario, rng_before.scenario);
    assert_eq!(sim.rng_state().mapgen, rng_before.mapgen);
    let terrain = sim.resolved_terrain.as_ref().unwrap();
    assert_eq!(
        terrain.cell(1, 2).unwrap().bridge_facts.raw_flags,
        crate::map::bridge_facts::BRIDGE_FLAG_DIRECTION_ZERO,
    );
    assert!(terrain.cell(2, 2).unwrap().overlay_blocks);
    assert!(!sim.path_grid().unwrap().is_walkable(2, 2));
}

#[test]
fn snapshot_restore_terrain_asset_error_stops_map_publication() {
    let (mut sim, rules, registry) = terrain_asset_restore_fixture();
    let mut expected_main = sim.main_rng.clone();
    let expected_word = expected_main.next_u32();
    let mut prepare =
        |terrain: &mut ResolvedTerrainGrid, main: &mut crate::sim::rng::SimRng, _: i32| {
            assert_eq!(terrain.cell(1, 2).unwrap().final_tile_index, 71);
            assert_eq!(main.next_u32(), expected_word);
            Err("missing selected TMP".to_string())
        };

    let error = sim
        .restore_map_authority_after_snapshot_load(&rules, &registry, Some(&mut prepare))
        .expect_err("a rejected candidate must not publish map authority");

    assert!(matches!(
        error,
        SnapshotRestoreError::TerrainAssetPreparationFailed { reason }
            if reason == "missing selected TMP"
    ));
    let terrain = sim.resolved_terrain.as_ref().unwrap();
    assert_eq!(terrain.cell(1, 2).unwrap().bridge_facts.raw_flags, 0);
    assert!(!terrain.cell(2, 2).unwrap().overlay_blocks);
    assert!(sim.path_grid().is_none());
    assert_eq!(sim.main_rng.logical_state(), expected_main.logical_state());
}

#[test]
fn snapshot_restore_rejects_truncated_overlay_cell_storage() {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::overlay_grid::{OverlayCell, OverlayGrid};

    #[derive(serde::Serialize)]
    struct OverlayGridWire {
        width: u16,
        height: u16,
        cells: Vec<OverlayCell>,
        retained_neighbor_counts: Vec<u8>,
    }

    let malformed_bytes = save_codec::serialize(&OverlayGridWire {
        width: 2,
        height: 1,
        cells: vec![OverlayCell::default()],
        retained_neighbor_counts: vec![0; 2],
    })
    .expect("malformed overlay wire fixture");
    let malformed: OverlayGrid =
        save_codec::deserialize(&malformed_bytes).expect("wire-compatible OverlayGrid");
    let ini = IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
         [OverlayTypes]\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("truncated-grid rules");
    let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);
    let mut sim = Simulation::new();
    *sim.overlay_grid = Some(malformed);
    *sim.resolved_terrain = Some(flat_terrain(2, 1));

    assert!(matches!(
        sim.restore_map_authority_after_snapshot_load(&rules, &registry, None),
        Err(SnapshotRestoreError::MapAuthorityCellStorageMismatch {
            expected: 2,
            found: 1,
        })
    ));
}

#[test]
fn snapshot_restore_rejects_truncated_retained_wall_neighbor_storage() {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::overlay_grid::{OverlayCell, OverlayGrid};

    #[derive(serde::Serialize)]
    struct OverlayGridWire {
        width: u16,
        height: u16,
        cells: Vec<OverlayCell>,
        retained_neighbor_counts: Vec<u8>,
    }

    let malformed_bytes = save_codec::serialize(&OverlayGridWire {
        width: 2,
        height: 1,
        cells: vec![OverlayCell::default(); 2],
        retained_neighbor_counts: vec![7],
    })
    .expect("malformed retained wall-neighbor wire fixture");
    let malformed: OverlayGrid =
        save_codec::deserialize(&malformed_bytes).expect("wire-compatible OverlayGrid");
    let ini = IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
         [OverlayTypes]\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("truncated-retained-grid rules");
    let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);
    let mut sim = Simulation::new();
    *sim.overlay_grid = Some(malformed);
    *sim.resolved_terrain = Some(flat_terrain(2, 1));

    assert!(matches!(
        sim.restore_map_authority_after_snapshot_load(&rules, &registry, None),
        Err(SnapshotRestoreError::RetainedWallNeighborStorageMismatch {
            expected: 2,
            found: 1,
        })
    ));
}

fn cell_order(sim: &Simulation, rx: u16, ry: u16, layer: MovementLayer) -> Vec<u64> {
    sim.substrate
        .occupancy
        .get(rx, ry)
        .map(|occ| occ.iter_layer(layer).map(|o| o.entity_id).collect())
        .unwrap_or_default()
}

#[test]
fn gsi_04_10_destroyed_terrain_snapshot_rebuild_does_not_resurrect_cache() {
    use crate::map::resolved_terrain::zone_class;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::pathfinding::zone_map::ZONE_INVALID;
    use crate::sim::terrain_object::{
        TerrainDamageResult, TerrainObjectLifecycle, TerrainObjectState,
        damage_terrain_object_at_cell, mark_terrain_occupation, mark_terrain_raw_occupation,
    };
    use crate::sim::terrain_spawn::TerrainAnimationState;

    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nTreeStrength=10\n\
         [InfantryTypes]\n\
         [VehicleTypes]\n0=DUMMY\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [TerrainTypes]\n0=TREE01\n1=TIBTRE01\n\
         [DUMMY]\nPrimary=Gun\nStrength=100\nArmor=heavy\n\
         [Gun]\nDamage=10\nWarhead=WH\n\
         [WH]\nWood=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n\
         [TREE01]\nStrength=10\nArmor=wood\nImmune=no\nTemperateOccupationBits=7\n\
         [TIBTRE01]\nStrength=10\nArmor=wood\nImmune=yes\nSpawnsTiberium=yes\nTemperateOccupationBits=7\n",
    ))
    .expect("terrain damage rules");
    let mut sim = Simulation::new();
    let damaged_cell = (0, 0);
    let destroyed_cell = (1, 1);
    let spawner_cell = (2, 2);
    let damaged_id = 1;
    let destroyed_id = 2;
    let spawner_id = 3;
    let tree_type = sim.interner.intern("TREE01");
    let spawner_type = sim.interner.intern("TIBTRE01");
    let damaged = {
        let mut terrain =
            TerrainObjectState::for_test(damaged_id, tree_type, damaged_cell.0, damaged_cell.1);
        terrain.set_health_for_test(6);
        terrain.set_max_health_for_test(10);
        terrain.set_occupation_bits_for_test(7);
        terrain
    };
    let destroyed = {
        let mut terrain = TerrainObjectState::for_test(
            destroyed_id,
            sim.interner.intern("TREE01"),
            destroyed_cell.0,
            destroyed_cell.1,
        );
        terrain.set_health_for_test(10);
        terrain.set_max_health_for_test(10);
        terrain.set_occupation_bits_for_test(7);
        terrain
    };
    let spawner = {
        let mut terrain =
            TerrainObjectState::for_test(spawner_id, spawner_type, spawner_cell.0, spawner_cell.1);
        terrain.set_health_for_test(10);
        terrain.set_max_health_for_test(10);
        terrain.set_occupation_bits_for_test(7);
        terrain
    };
    for terrain in [&damaged, &destroyed, &spawner] {
        sim.terrain_objects
            .insert_constructed(terrain.clone(), terrain.stable_id() == spawner_id);
        sim.terrain_objects
            .add_to_cell(terrain.stable_id(), terrain.cell());
        assert!(sim.register_terrain_object(terrain.stable_id(), &rules));
        mark_terrain_raw_occupation(
            &mut sim.substrate.raw_cell_occupation,
            terrain.cell(),
            terrain.occupation_bits(),
        );
    }
    sim.set_next_stable_object_id_for_test(spawner_id + 1);
    sim.terrain_objects.animations_mut_for_test().insert(
        spawner_id,
        TerrainAnimationState::new(
            spawner_type,
            crate::util::native_x87::NativeF32Bits::from_bits(0x3b44_9ba6),
            3,
            22,
            sim.session.binary_frame(),
        ),
    );
    sim.terrain_objects
        .spawning_cells_mut_for_test()
        .insert(spawner_cell);

    let mut original_occupied_grid = flat_terrain(3, 3);
    for terrain in [&damaged, &destroyed, &spawner] {
        mark_terrain_occupation(terrain, Some(&mut original_occupied_grid));
    }
    let stale_original_grid = original_occupied_grid.clone();
    assert!(
        !PathGrid::from_resolved_terrain(&stale_original_grid)
            .is_walkable(destroyed_cell.0, destroyed_cell.1)
    );
    *sim.resolved_terrain = Some(original_occupied_grid);

    let result = damage_terrain_object_at_cell(
        &mut sim.terrain_objects,
        &mut sim.substrate.raw_cell_occupation,
        &rules,
        &sim.interner,
        destroyed_cell,
        10,
        rules.warhead("WH").expect("wood warhead"),
        sim.resolved_terrain.as_mut(),
    );
    assert_eq!(result, TerrainDamageResult::Destroyed);
    assert!(!sim.unregister_non_entity_object(destroyed_id));
    assert_eq!(sim.substrate.display.layer_of(destroyed_id), None);
    sim.substrate.pending_delete.queue_object(destroyed_id);
    // Native in-scenario load restarts Scenario RNG from Seed0; isolate
    // destroyed-terrain persistence on that same post-load cursor.
    *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    assert!(sim.rebuild_dynamic_navigation(&rules));
    let authoritative_hash = sim.state_hash();

    let bytes = GameSnapshot::save(&sim, 0, 0, "gsi_04_10_destroyed_terrain", 0);
    let mut restored = GameSnapshot::load(&bytes)
        .expect("destroyed Terrain snapshot")
        .sim;
    restored
        .restore_after_snapshot_load()
        .expect("restore serialized authority");
    restored.rebuild_caches_after_load(
        stale_original_grid,
        crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
        &rules,
    );

    assert_eq!(
        restored.terrain_objects.objects()[&destroyed_id].lifecycle(),
        TerrainObjectLifecycle::Destroyed
    );
    assert!(
        !restored
            .terrain_objects
            .cell_index()
            .contains_key(&destroyed_cell)
    );
    let restored_damaged = &restored.terrain_objects.objects()[&damaged_id];
    assert_eq!(restored_damaged.health(), 6);
    assert_eq!(restored_damaged.lifecycle(), TerrainObjectLifecycle::Live);
    assert_eq!(restored_damaged.cell(), damaged_cell);
    assert_eq!(
        restored.terrain_objects.cell_index()[&damaged_cell],
        damaged_id
    );
    let restored_spawner = &restored.terrain_objects.objects()[&spawner_id];
    assert_eq!(restored_spawner.health(), 10);
    assert_eq!(restored_spawner.lifecycle(), TerrainObjectLifecycle::Live);
    assert_eq!(restored_spawner.cell(), spawner_cell);
    assert_eq!(
        restored.terrain_objects.cell_index()[&spawner_cell],
        spawner_id
    );
    assert_eq!(
        restored.terrain_objects.animations()[&spawner_id].type_ref(),
        spawner_type
    );
    assert!(
        restored
            .terrain_objects
            .spawning_cells()
            .contains(&spawner_cell)
    );
    assert_eq!(restored.next_stable_object_id(), spawner_id + 1);
    let reconciled = restored
        .resolved_terrain
        .as_ref()
        .expect("reconciled terrain");
    let source = reconciled
        .cell(destroyed_cell.0, destroyed_cell.1)
        .expect("source cell");
    assert_eq!(source.terrain_object_occupation, None);
    assert!(!source.terrain_object_blocks);
    assert!(!source.ground_walk_blocked);
    assert_eq!(source.zone_type, zone_class::GROUND);
    for (cell, bits) in [(damaged_cell, 7), (spawner_cell, 7)] {
        let live = reconciled.cell(cell.0, cell.1).expect("live Terrain cell");
        assert_eq!(live.terrain_object_occupation, Some(bits));
        assert!(live.terrain_object_blocks);
        assert!(live.ground_walk_blocked);
        let raw_mask = crate::sim::terrain_object::terrain_raw_occupation_mask(bits);
        assert_eq!(
            restored
                .substrate
                .raw_cell_occupation
                .ground_bits(cell.0, cell.1)
                & raw_mask,
            raw_mask
        );
    }
    assert_eq!(
        restored
            .substrate
            .raw_cell_occupation
            .ground_bits(destroyed_cell.0, destroyed_cell.1)
            & crate::sim::terrain_object::terrain_raw_occupation_mask(destroyed.occupation_bits(),),
        0
    );

    let path =
        PathGrid::from_resolved_terrain_with_bridges(reconciled, restored.overlay_grid.as_ref());
    assert!(path.is_walkable(destroyed_cell.0, destroyed_cell.1));
    assert!(!path.is_walkable(damaged_cell.0, damaged_cell.1));
    assert!(!path.is_walkable(spawner_cell.0, spawner_cell.1));
    assert!(restored.rebuild_dynamic_navigation(&rules));
    assert_ne!(
        restored
            .zone_grid
            .as_ref()
            .and_then(|zones| zones.map_for(MovementZone::Normal))
            .expect("normal zone map")
            .zone_at(destroyed_cell.0, destroyed_cell.1, MovementLayer::Ground),
        ZONE_INVALID
    );
    assert_eq!(restored.state_hash(), authoritative_hash);

    let mut changed_health = GameSnapshot::load(&bytes)
        .expect("health-mutation control snapshot")
        .sim;
    changed_health
        .terrain_objects
        .objects_mut_for_test()
        .get_mut(&damaged_id)
        .expect("damaged Terrain authority")
        .set_health_for_test(5);
    assert_ne!(changed_health.state_hash(), authoritative_hash);

    let mut changed_lifecycle = GameSnapshot::load(&bytes)
        .expect("lifecycle-mutation control snapshot")
        .sim;
    changed_lifecycle
        .terrain_objects
        .objects_mut_for_test()
        .get_mut(&spawner_id)
        .expect("spawning Terrain authority")
        .set_lifecycle_for_test(TerrainObjectLifecycle::Limbo);
    assert_ne!(changed_lifecycle.state_hash(), authoritative_hash);
}

#[test]
fn saveload_rebuild_is_deterministic() {
    use crate::map::entities::EntityCategory;
    use crate::sim::game_entity::GameEntity;

    let terrain = flat_terrain(8, 8);
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Americans");
    for (stable_id, type_id, category, rx, ry) in [
        (3, "GAPOWR", EntityCategory::Structure, 2, 2),
        (1, "MTNK", EntityCategory::Unit, 3, 2),
        (2, "E1", EntityCategory::Infantry, 3, 2),
    ] {
        let mut entity = GameEntity::test_default(stable_id, type_id, "Americans", rx, ry);
        entity.set_owner_for_test(owner);
        entity.set_category_for_test(category);
        if category == EntityCategory::Infantry {
            entity.set_sub_cell(Some(2));
        }
        sim.substrate.entities.insert(entity);
        sim.add_entity_occupancy(stable_id);
    }
    let bytes = GameSnapshot::save(&sim, 0, 0, "deterministic_rebuild", 0);

    let mut a = GameSnapshot::load(&bytes)
        .expect("first load should succeed")
        .sim;
    let mut b = GameSnapshot::load(&bytes)
        .expect("second load should succeed")
        .sim;
    rebuild_load_caches(&mut a, terrain.clone());
    rebuild_load_caches(&mut b, terrain);

    assert_eq!(cell_order(&a, 3, 2, MovementLayer::Ground), vec![2, 1]);
    assert_eq!(
        cell_order(&a, 3, 2, MovementLayer::Ground),
        cell_order(&b, 3, 2, MovementLayer::Ground)
    );

    let path_a = PathGrid::from_resolved_terrain_with_bridges(
        a.resolved_terrain.as_ref().expect("terrain restored"),
        a.overlay_grid.as_ref(),
    );
    let path_b = PathGrid::from_resolved_terrain_with_bridges(
        b.resolved_terrain.as_ref().expect("terrain restored"),
        b.overlay_grid.as_ref(),
    );
    assert_eq!(path_a, path_b);

    a.rebuild_zone_grid(&path_a);
    b.rebuild_zone_grid(&path_b);
    assert_zone_grids_equivalent(
        a.zone_grid.as_ref().expect("zone grid rebuilt"),
        b.zone_grid.as_ref().expect("zone grid rebuilt"),
    );
    assert_eq!(a.state_hash(), b.state_hash());
}

/// A saved Terrain's type id and the interner that names it both come from the
/// file. An id past the interner is refused as a missing type, like an unknown
/// name, instead of panicking in the name lookup.
#[test]
fn terrain_type_id_past_the_saved_interner_is_a_missing_type() {
    let mut sim = Simulation::new();
    let unknown = crate::sim::intern::InternedId::from_index(sim.interner.len() as u32);
    sim.terrain_objects.insert_constructed(
        crate::sim::terrain_object::TerrainObjectState::for_test(1, unknown, 0, 0),
        false,
    );
    let rules = crate::rules::ruleset::RuleSet::empty_for_test();
    assert_eq!(
        sim.restore_map_authority_after_snapshot_load(rules, rules.overlay_registry(), None)
            .unwrap_err(),
        SnapshotRestoreError::MissingTerrainType {
            object_id: 1,
            type_name: format!("#{}", unknown.index()),
        },
    );
}
