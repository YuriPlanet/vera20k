//! Native Terrain placement coordinates retained through map changes and saves.
use super::*;
use crate::map::overlay::TerrainObject;
use crate::sim::combat::combat_aoe::{
    AoEAirImpact, AoELayerContext, AreaDamageReceiver, TerrainCollectionView,
    apply_aoe_damage_with_terrain,
};
use crate::sim::occupancy::OccupancyGrid;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::Simulation;
use crate::util::lepton::CELL_CENTER_LEPTON;
use std::hash::{Hash, Hasher};

#[test]
fn retail_signed_terrain_dummy_suffix_survives_save_and_limbo_without_replacement_membership() {
    use crate::map::overlay::OverlayEntry;
    use crate::map::resolved_terrain::SharedCellDummy;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::movement::locomotor::MovementLayer;
    use crate::sim::occupancy::RawCellKey;
    use crate::sim::overlay_grid::OverlayGrid;
    use crate::sim::terrain_spawn::{
        TerrainConstructionMode, TerrainConstructionRules, construct_terrain_objects_with_rules,
    };
    use crate::sim::world::UninitContext;

    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = battle.rules;
    let registry = rules.overlay_registry();
    let ore = registry.id_for_name("TIB01").unwrap();
    let mut sim = Simulation::with_seed(31);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let mut grid = crate::map::resolved_terrain::test_flat_ground_grid(8);
    // Real control and its eight neighbors; source0 and its halo are absent,
    // as on XXmas's allocation diamond. Dense cell0 would exercise another path.
    grid.test_set_native_allocated_cells(&[
        (2, 2),
        (3, 2),
        (4, 2),
        (2, 3),
        (3, 3),
        (4, 3),
        (2, 4),
        (3, 4),
        (4, 4),
    ]);
    sim.install_resolved_terrain_for_new_map(grid.clone());
    sim.overlay_grid = Some(OverlayGrid::from_overlay_entries(
        &[OverlayEntry {
            rx: 0,
            ry: 0,
            overlay_id: ore,
            frame: 7,
        }],
        8,
        8,
    ));
    sim.native_unique_ids =
        Some(crate::sim::native_identity::NativeUniqueIdCursor::for_synthetic_simulation());
    let dummy = sim.resolved_terrain.as_ref().unwrap().shared_cell_dummy();
    dummy.write_overlay_identity_state(i32::from(ore), 9);
    let rng = sim.rng_state();
    let objects = crate::map::overlay::parse_terrain_objects(
        &IniFile::from_str("[Terrain]\n-1=TREE01\n3003=TREE01\n"),
        4,
    );
    assert_eq!(
        construct_terrain_objects_with_rules(
            &mut sim,
            &objects,
            &mut TerrainConstructionRules::Projected(&rules),
            true,
            TerrainConstructionMode::Authored(registry)
        )
        .unwrap(),
        2
    );
    let tree = sim.terrain_objects.objects().values().next().unwrap();
    let id = tree.stable_id();
    let retained = tree.clone();
    assert_eq!(
        tree.world_coord(),
        crate::sim::projectile::ProjectileCoord::new(-128, 128, 0)
    );
    assert_eq!(tree.cell(), (0, 0));
    assert_eq!(sim.terrain_objects.cell_index().len(), 1);
    assert_eq!(
        dummy.neighbor_count(),
        8,
        "Unlimbo71D080..71D097 writes all eight misses"
    );
    assert_eq!(dummy.overlay_identity_state(), (-1, 0));
    assert_eq!(
        sim.overlay_grid.as_ref().unwrap().cell(0, 0).overlay_id,
        Some(ore),
        "Dummy resource clearing must not write a real slot"
    );
    assert_eq!(sim.rng_state(), rng);
    // Compute_Game_Sync_Checksum64DD1B reads raw Location, including the
    // live Display nonmember, rather than recentering Get_Cell_Packed.
    use crate::sim::multiplayer_checksum::{ChecksumObject, RetailChecksumAccumulator};
    let mut expected = RetailChecksumAccumulator::new();
    expected.fold_object(ChecksumObject::new(-128, 128, 0x24, 0));
    expected.fold_object(ChecksumObject::new(896, 896, 0x24, 0));
    let mut draws = sim.scenario_rng.clone();
    expected.fold_value(draws.next_u32());
    let checksum = sim.compute_retail_multiplayer_checksum().unwrap();
    assert_eq!(checksum.value, expected.value());
    assert_eq!(checksum.diagnostic_rng_sample, draws.next_u32());
    let plane = crate::sim::movement::bump_crush::build_blocker_neighbor_counts_with_overlays(
        &sim.substrate.entities,
        8,
        8,
        sim.resolved_terrain.as_ref(),
        &sim.terrain_objects,
        sim.overlay_grid.as_ref(),
        &sim.interner,
        &rules,
    );
    assert_eq!(
        plane.count_at(0, 1),
        0,
        "missing neighbor slots have no real counter"
    );
    assert_eq!(plane.count_at(2, 3), 1);

    let cursor = sim.native_identity_cursor();
    let bytes =
        GameSnapshot::save_validated(&sim, 0, rules.simulation_config_hash(), "signed Terrain", 0);
    let mut loaded = GameSnapshot::load(&bytes).unwrap().sim;
    // Same Resize/reset and restoration owners as PreparedLoad::prepare_snapshot;
    // loading restores actual saved heads, never normal Terrain Unlimbo.
    loaded.bind_shared_cell_dummy(SharedCellDummy::fresh());
    loaded.reconstruct_cellclass_dummy_for_map_resize();
    loaded.restore_after_snapshot_load().unwrap();
    loaded.rebuild_caches_after_load(grid, Default::default(), &rules);
    loaded
        .restore_map_authority_after_snapshot_load(&rules, registry, None)
        .unwrap();
    loaded.resolve_type_handles(&rules);
    assert_eq!(loaded.terrain_objects.get(id), Some(&retained));
    assert_eq!(loaded.native_identity_cursor(), cursor);
    assert_eq!(loaded.terrain_objects.cell_index().len(), 1);
    let dummy = loaded
        .resolved_terrain
        .as_ref()
        .unwrap()
        .shared_cell_dummy();
    assert_eq!(dummy.neighbor_count(), 0, "load must not replay the +8");
    let owner = loaded.interner.intern("Americans");
    loaded.substrate.raw_cell_occupation.write_occupant(
        RawCellKey::Dummy,
        MovementLayer::Ground,
        0x43,
        Some(owner),
        true,
    );
    loaded.dirty_cells.clear_radar_for_test();
    loaded.uninit_with_context(id, UninitContext::with_rules(&rules));
    assert_eq!(
        dummy.neighbor_count(),
        248,
        "Limbo71C9A1..71C9B8 decrements the current byte"
    );
    assert_eq!(
        loaded
            .substrate
            .raw_cell_occupation
            .occupant_at(RawCellKey::Dummy, MovementLayer::Ground),
        (3, Some(owner)),
        "71C9F2 is a literal bit clear, retaining the owner"
    );
    assert_eq!(dummy.snapshot().coord, (0, 0));
    assert_eq!(loaded.dirty_cells.radar(), [(0, 0)]);
    assert!(!loaded.terrain_objects.get(id).unwrap().object_alive());
    assert!(loaded.substrate.display.layer_of(id).is_none());
    loaded.process_pending_delete_with(&rules, Some(registry), Default::default(), 1, true);
    assert!(loaded.terrain_objects.get(id).is_none());
    assert_eq!(loaded.terrain_objects.cell_index().len(), 1);
    assert_eq!(
        dummy.neighbor_count(),
        248,
        "destructor Limbo's unconditional suffix must not decrement again"
    );
}

#[test]
fn bridge_neighbor_terrain_retains_native_surface_coords_through_damage_and_restore() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
        return;
    };
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/terrain_coordinate.json",
    ))
    .unwrap();
    for row in corpus["cases"].as_array().unwrap() {
        let input = &row["input"];
        // Authored Terrain construction always supplies inputZ0. The extra
        // native above-ground input characterizes a different caller domain.
        if input["input_z"] != 0 {
            continue;
        }
        let mut sim = Simulation::with_seed(1);
        let mut grid = crate::map::resolved_terrain::test_flat_ground_grid(32);
        let cell = grid.cell_mut_for_test(10, 20).unwrap();
        cell.level = input["level"].as_i64().unwrap() as i8 as u8;
        cell.slope_type = input["slope"].as_u64().unwrap() as u8;
        cell.bridge_facts.raw_flags = input["flags"].as_u64().unwrap() as u32;
        sim.install_resolved_terrain_for_new_map(grid);
        crate::sim::terrain_spawn::construct_terrain_objects(
            &mut sim,
            &[TerrainObject {
                rx: 10,
                ry: 20,
                name: "TREE01".into(),
            }],
            &rules,
            false,
        );
        let id = sim.terrain_objects.cell_index()[&(10, 20)];
        let coord = sim.terrain_objects.objects()[&id].world_coord();
        assert_eq!(
            serde_json::json!([coord.x, coord.y, coord.z]),
            row["placement"],
            "{input}"
        );
        let mut higher = sim.terrain_objects.objects()[&id].clone();
        if let TerrainLocation::Centered { z_leptons, .. } = &mut higher.location {
            *z_leptons += 1;
        }
        let hash = |tree: &TerrainObjectState| {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            tree.hash(&mut hasher);
            hasher.finish()
        };
        assert_ne!(hash(&higher), hash(&sim.terrain_objects.objects()[&id]));

        let cell = sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut_for_test(10, 20)
            .unwrap();
        cell.level = 7;
        cell.slope_type = 0;
        cell.bridge_facts.raw_flags = 0;
        assert_eq!(
            serde_json::json!([
                coord.x,
                coord.y,
                sim.terrain_objects.objects()[&id].world_coord().z
            ]),
            row["after_ground_change"],
            "{input}"
        );
        let changed_grid = sim.resolved_terrain.as_ref().unwrap().clone();
        let bytes = GameSnapshot::save(&sim, 0, 0, "terrain-coordinate", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        restored.rebuild_caches_after_load(
            changed_grid,
            crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
            &rules,
        );
        assert_eq!(
            restored.terrain_objects.objects()[&id].world_coord(),
            coord,
            "{input}"
        );

        // Consumer regression: an impact at the native retained coordinate
        // must still find the object after the ground under it has moved.
        let warhead = rules.warhead("HE").unwrap();
        let warhead_ref = restored.interner.intern("HE");
        let occupancy = OccupancyGrid::new();
        let result = apply_aoe_damage_with_terrain(
            &mut restored.substrate.entities,
            10,
            20,
            20,
            warhead,
            &rules,
            &mut restored.interner,
            (crate::sim::combat::RAD_NO_ATTACKER, None, warhead_ref),
            AoELayerContext {
                occupancy: Some(&occupancy),
                terrain: restored.resolved_terrain.as_mut(),
                air_impact: Some(AoEAirImpact {
                    sub_x: CELL_CENTER_LEPTON,
                    sub_y: CELL_CENTER_LEPTON,
                    z_leptons: coord.z,
                }),
                ..AoELayerContext::default()
            },
            Some(TerrainCollectionView {
                store: &restored.terrain_objects,
            }),
        );
        let terrain_hit = result
            .receivers
            .iter()
            .find_map(|receiver| match receiver {
                AreaDamageReceiver::Terrain(hit) if hit.stable_id == id => Some(hit),
                _ => None,
            })
            .expect("area collector must use the retained object coordinate");
        assert_eq!(terrain_hit.distance_leptons, 0, "{input}");
    }
}
