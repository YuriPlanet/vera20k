//! Snapshot round trips of per-object runtime state: Techno, Foot, Building
//! and cargo fields, building animation slots, damage and mission schema,
//! each checked against the restored state hash.

use super::*;

#[test]
fn signed_actual_and_estimated_health_survive_save_restore_independently() {
    for (actual, estimated) in [(-7, 100_000), (100_000, -19), (i32::MIN, i32::MAX)] {
        let mut sim = Simulation::new();
        assert_eq!(sim.substrate.allocate_stable_id(), 1);
        let mut entity =
            crate::sim::game_entity::GameEntity::test_default(1, "SIGNED", "Americans", 0, 0);
        entity.set_type_ref_for_test(sim.intern("SIGNED"));
        entity.set_owner_for_test(sim.intern("Americans"));
        entity.health.set_for_test(actual);
        entity.estimated_health =
            crate::sim::estimated_health::EstimatedHealth::from_raw(estimated);
        sim.substrate.entities.insert(entity);
        // Native load resets Scenario RNG to seed zero; isolate the health
        // roundtrip from that intentional load transition.
        *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let before = sim.state_hash();
        let bytes = GameSnapshot::save(&sim, 0, 0, "signed-health", 0);
        let mut restored = GameSnapshot::load(&bytes)
            .expect("signed health schema")
            .sim;
        restored
            .restore_after_snapshot_load()
            .expect("restore signed health");
        let entity = restored.substrate.entities.get(1).unwrap();
        assert_eq!(
            (entity.health.current(), entity.estimated_health.get()),
            (actual, estimated)
        );
        assert_eq!(restored.state_hash(), before);
        restored
            .substrate
            .entities
            .get_mut(1)
            .unwrap()
            .health
            .set_for_test(actual.wrapping_add(1));
        assert_ne!(restored.state_hash(), before);
        restored
            .substrate
            .entities
            .get_mut(1)
            .unwrap()
            .health
            .set_for_test(actual);
        restored
            .substrate
            .entities
            .get_mut(1)
            .unwrap()
            .estimated_health =
            crate::sim::estimated_health::EstimatedHealth::from_raw(estimated.wrapping_add(1));
        assert_ne!(restored.state_hash(), before);
    }
}

#[test]
fn fractional_passenger_sizes_roundtrip_and_recompute_after_remove() {
    use crate::sim::game_entity::GameEntity;
    use crate::sim::passenger::{PassengerCargo, PassengerRole};

    let oracle: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/cargo_total_size.json",
    ))
    .unwrap();
    let row = oracle["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == "positive_then_negative")
        .unwrap();
    let size_bits: Vec<u64> = row["input"]["size_f64_bits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|bits| u64::from_str_radix(bits.as_str().unwrap(), 16).unwrap())
        .collect();
    let sizes: Vec<f64> = size_bits.iter().copied().map(f64::from_bits).collect();
    let native_total = row["total_size_i32"].as_i64().unwrap() as i32;

    let mut sim = Simulation::new();
    let carrier_id = sim.substrate.allocate_stable_id();
    let passenger_ids = [
        sim.substrate.allocate_stable_id(),
        sim.substrate.allocate_stable_id(),
    ];
    let mut cargo = PassengerCargo::new(2, 1.25);
    cargo.board_forced_chain(&passenger_ids.into_iter().zip(sizes).collect::<Vec<_>>());
    assert_eq!(cargo.total_size(), native_total);

    let owner = sim.intern("Americans");
    let mut carrier = GameEntity::test_default(carrier_id, "TRAN", "Americans", 0, 0);
    carrier.set_owner_for_test(owner);
    carrier.set_type_ref_for_test(sim.intern("TRAN"));
    carrier.set_passenger_role_for_test(PassengerRole::transport(cargo));
    sim.substrate.entities.insert(carrier);
    for id in passenger_ids {
        let mut passenger = GameEntity::test_default(id, "E1", "Americans", 0, 0);
        passenger.set_owner_for_test(owner);
        passenger.set_type_ref_for_test(sim.intern("E1"));
        passenger
            .passenger_role_mut_for_test()
            .set_inside_transport(carrier_id, false);
        sim.substrate.entities.insert(passenger);
    }

    *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let before = sim.state_hash();
    let bytes = GameSnapshot::save(&sim, 0, 0, "fractional-cargo", 0);
    let mut restored = GameSnapshot::load(&bytes)
        .expect("fractional cargo schema")
        .sim;
    restored
        .restore_after_snapshot_load()
        .expect("fractional cargo references");
    let cargo = restored
        .substrate
        .entities
        .get(carrier_id)
        .unwrap()
        .passenger_role()
        .cargo()
        .unwrap();
    assert_eq!(cargo.passengers(), passenger_ids.as_slice());
    assert_eq!(
        cargo
            .passenger_sizes()
            .iter()
            .map(|size| size.to_bits())
            .collect::<Vec<_>>(),
        size_bits
    );
    assert_eq!(cargo.size_limit().to_bits(), 1.25_f64.to_bits());
    assert_eq!(cargo.total_size(), native_total);
    assert_eq!(cargo.total_size(), cargo.recomputed_total_size());
    assert_eq!(restored.state_hash(), before);

    let cargo = restored
        .substrate
        .entities
        .get_mut(carrier_id)
        .unwrap()
        .passenger_role_mut_for_test()
        .cargo_mut()
        .unwrap();
    assert!(cargo.disembark(passenger_ids[1]));
    assert_eq!(cargo.passengers(), &passenger_ids[..1]);
    assert_eq!(cargo.passenger_sizes()[0].to_bits(), size_bits[0]);
    assert_eq!(cargo.total_size(), cargo.recomputed_total_size());
    assert_eq!(cargo.total_size(), 1);
    restored
        .substrate
        .entities
        .get_mut(passenger_ids[1])
        .unwrap()
        .passenger_role_mut_for_test()
        .clear_carrier_relation();
    assert_ne!(restored.state_hash(), before);
}

#[test]
fn restore_rejects_a_live_infantryman_with_the_dying_mark() {
    use crate::map::entities::EntityCategory;
    use crate::sim::game_entity::GameEntity;
    for (category, object_alive, rejected) in [
        (EntityCategory::Infantry, true, true),
        (EntityCategory::Infantry, false, false),
        (EntityCategory::Unit, true, false),
    ] {
        let mut sim = Simulation::new();
        let mut entity = GameEntity::test_default(1, "E1", "Americans", 5, 5);
        entity.set_category_for_test(category);
        entity.death.set_dying_for_test(true);
        entity.lifecycle.set_object_alive_for_test(object_alive);
        sim.substrate.entities.insert(entity);
        let bytes = GameSnapshot::save(&sim, 0, 0, "test_map", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        assert_eq!(
            restored.restore_after_snapshot_load()
                == Err(SnapshotRestoreError::InvalidInfantryDyingMark { object_id: 1 }),
            rejected,
            "{category:?} alive={object_alive}"
        );
    }
}

#[test]
fn authored_wall_neighbor_authority_roundtrips_v115() {
    let mut sim = Simulation::new();
    *sim.overlay_grid = Some(
        crate::sim::overlay_grid::OverlayGrid::from_finalized_map_payload(
            crate::map::authored_overlay::FinalizedOverlayPayload::from_cells_for_test(
                2,
                2,
                vec![(-1, 0), (2, 3), (-1, 0), (-1, 0)],
                vec![0, 7, 255, 4],
            ),
        ),
    );

    let bytes = GameSnapshot::save(&sim, 0, 0, "authored-wall-counts", 0);
    let restored = GameSnapshot::load(&bytes)
        .expect("current v115 snapshot")
        .sim;
    assert_eq!(
        restored
            .overlay_grid
            .as_ref()
            .expect("overlay authority")
            .retained_neighbor_counts(),
        &[0u8, 7, 255, 4][..]
    );
}

#[test]
fn overlay_rubble_building_type_roundtrips() {
    let mut sim = Simulation::new();
    let ruin = sim.interner.intern("CAOUTP");
    let mut grid = crate::sim::overlay_grid::OverlayGrid::new(3, 3);
    grid.place_overlay(1, 2, 0xEF, 4);
    grid.cell_mut_for_test(1, 2).rubble_building_type = Some(ruin);
    grid.place_overlay(2, 2, 0xEF, 0);
    *sim.overlay_grid = Some(grid);

    let bytes = GameSnapshot::save(&sim, 0, 0, "rubble", 0);
    let restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
    let grid = restored.overlay_grid.as_ref().expect("overlay authority");
    assert_eq!(grid.cell(1, 2).rubble_building_type, Some(ruin));
    assert_eq!(grid.cell(1, 2).overlay_data, 4);
    assert_eq!(grid.cell(2, 2).rubble_building_type, None);
}

#[test]
fn phase14_crate_authority_roundtrips_visible_and_ghost_slots() {
    let mut sim = Simulation::with_seed(0);
    *sim.crate_authority_mut_for_test().slot_mut_for_test(0) = crate::sim::crates::CrateSlot {
        start_frame: 17,
        aux: 0x40b5_1800,
        duration: 3375,
        cell_x: 6,
        cell_y: 9,
    };
    *sim.crate_authority_mut_for_test().slot_mut_for_test(1) = crate::sim::crates::CrateSlot {
        start_frame: -2,
        aux: u32::MAX,
        duration: i32::MIN,
        cell_x: -4,
        cell_y: 12,
    };
    let expected = sim.crate_authority().clone();

    let bytes = GameSnapshot::save(&sim, 0, 0, "test_map", 0);
    let loaded = GameSnapshot::load(&bytes).expect("v114 crate authority snapshot");
    assert_eq!(loaded.sim.crate_authority(), &expected);
}

#[test]
fn drive_ship_active_and_stashed_slope_phase_roundtrip_without_resample_or_rng() {
    use crate::map::entities::EntityCategory;
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::locomotion::LocomotorRuntimePayload;
    use crate::sim::movement::locomotor::LocomotorState;

    // Full snapshot load restarts Scenario RNG from Seed0. Start on that
    // canonical cursor so the slope-only round trip can prove no draw.
    let mut sim = Simulation::with_seed(0);
    sim.session.set_binary_frame_for_test(51);
    let mut entity = GameEntity::test_default(1, "SLOPE", "Americans", 0, 0);
    entity.set_owner_for_test(sim.intern("Americans"));
    entity.set_type_ref_for_test(sim.intern("SLOPE"));
    entity.set_category_for_test(EntityCategory::Unit);
    let mut locomotor = LocomotorState::for_test_kind_at_frame(LocomotorKind::Drive, 40);
    let drive = locomotor.active_slope_transition_mut().unwrap();
    drive.snap(2, 40);
    drive.sample_process_entry(7, 49);
    assert!(locomotor.begin_piggyback(LocomotorKind::Ship, 50));
    let ship = locomotor.active_slope_transition_mut().unwrap();
    ship.snap(4, 40);
    ship.sample_process_entry(9, 49);
    entity.locomotor = Some(locomotor);
    sim.substrate.entities.insert(entity);

    let before_hash = sim.state_hash();
    let before_rng = sim.scenario_rng.logical_state();
    let before = sim
        .substrate
        .entities
        .get(1)
        .unwrap()
        .locomotor
        .as_ref()
        .unwrap();
    let active_before = before.runtime_payload().clone();
    let stashed_before = before.piggyback().clone();

    let bytes = GameSnapshot::save(&sim, 1, 2, "Drive Ship slope", 3);
    let mut restored = GameSnapshot::load(&bytes)
        .expect("current v107 slope snapshot")
        .sim;
    let loaded = restored
        .substrate
        .entities
        .get(1)
        .unwrap()
        .locomotor
        .as_ref()
        .unwrap();
    assert_eq!(*loaded.runtime_payload(), active_before);
    assert_eq!(*loaded.piggyback(), stashed_before);
    assert_eq!(restored.session.binary_frame(), 51);
    assert_eq!(restored.state_hash(), before_hash);
    assert_eq!(restored.scenario_rng.logical_state(), before_rng);
    assert_eq!(
        loaded.active_slope_transition().unwrap().hash_fields(),
        (4, 9, 49, 3),
        "the saved active timer starts two committed frames before session frame 51"
    );
    assert!(matches!(
        loaded.piggyback().as_deref().map(|stashed| stashed.runtime_payload()),
        Some(LocomotorRuntimePayload::Drive(state))
            if state.slope().hash_fields() == (2, 7, 49, 3)
    ));

    let mut live_cell = crate::map::resolved_terrain::test_clear_cell(0, 0);
    live_cell.slope_type = 12;
    let live_terrain = ResolvedTerrainGrid::from_cells(1, 1, vec![live_cell]);
    *restored.resolved_terrain = Some(live_terrain.clone());
    assert_eq!(
        restored
            .substrate
            .entities
            .get(1)
            .unwrap()
            .locomotor
            .as_ref()
            .unwrap()
            .active_slope_transition()
            .unwrap()
            .hash_fields(),
        (4, 9, 49, 3),
        "load/rebuild trusts the saved cache instead of resampling live terrain"
    );

    let mut sound_events = Vec::new();
    crate::sim::movement::tick_movement_with_grids(
        &mut restored.substrate.entities,
        Some(&[1]),
        None,
        &Default::default(),
        &mut restored.substrate.occupancy,
        &mut restored.substrate.raw_cell_occupation,
        &mut restored.scenario_rng,
        52,
        52,
        None,
        Some(&live_terrain),
        None,
        &crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
        &mut restored.interner,
        None,
        &mut sound_events,
    );
    assert_eq!(
        restored
            .substrate
            .entities
            .get(1)
            .unwrap()
            .locomotor
            .as_ref()
            .unwrap()
            .active_slope_transition()
            .unwrap()
            .hash_fields(),
        (9, 12, 52, 3),
        "only the next eligible Process restarts against live terrain"
    );
    assert_eq!(restored.scenario_rng.logical_state(), before_rng);
}

#[test]
fn building_loss_index_survives_transfer_and_snapshot_with_its_full_hash_value() {
    use crate::map::entities::EntityCategory;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::intern::test_intern;

    let mut hashes = std::collections::BTreeSet::new();
    for retained in [0, 3, 11, -1] {
        let mut sim = Simulation::new();
        *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let mut building = GameEntity::test_default_of_category(
            1,
            "GAPOWR",
            "Americans",
            4,
            5,
            EntityCategory::Structure,
        );
        match retained {
            0 => {}
            3 => building.initialize_building_loss_index(3),
            11 => building.record_building_damage_house_index(11),
            -1 => building.suppress_building_sale_loss(),
            _ => unreachable!(),
        }
        sim.substrate.entities.insert(building);
        sim.substrate
            .entities
            .change_owner(1, test_intern("Russians"));
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().building_loss_index(),
            retained,
            "owner transfer must not recompute the retained index"
        );
        let expected_hash = sim.state_hash();
        assert!(
            hashes.insert(expected_hash),
            "hash the full index, not only whether it suppresses loss"
        );
        let rng = sim.scenario_rng.logical_state();
        let bytes = GameSnapshot::save(&sim, 1, 2, "building_loss_index", 0);
        let restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
        assert_eq!(
            restored
                .substrate
                .entities
                .get(1)
                .unwrap()
                .building_loss_index(),
            retained
        );
        assert_eq!(restored.state_hash(), expected_hash);
        assert_eq!(restored.scenario_rng.logical_state(), rng);
    }
}

#[test]
fn techno_constructor_word_and_installed_upgrades_roundtrip_without_a_draw_and_hash() {
    let mut sim = Simulation::new();
    let mut entity = crate::sim::game_entity::GameEntity::test_default(1, "UP1", "Americans", 4, 5);
    entity.set_techno_ctor_random_word_for_test(0xA55A);
    let upgrade_type = sim.interner.intern("UP1");
    entity.set_building_upgrades_for_test(3, [None, None, Some(upgrade_type)]);
    sim.substrate.entities.insert(entity);
    let rules = crate::rules::ruleset::RuleSet::from_ini(
        &crate::rules::ini_parser::IniFile::from_str("[BuildingTypes]\n0=UP1\n[UP1]\nUpgrades=3\n"),
    )
    .unwrap();
    sim.set_building_upgrade_art_name(upgrade_type, 0, "ART_UP", &rules);
    // Native load resets Scenario RNG to Seed(0); align the source before
    // comparing persistence/hash state unrelated to that separate rule.
    *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let source_rng = sim.scenario_rng.logical_state();
    let source_hash = sim.state_hash();

    let bytes = GameSnapshot::save(&sim, 0, 0, "techno-constructor", 0);
    let restored = GameSnapshot::load(&bytes).unwrap().sim;
    let restored_entity = restored.substrate.entities.get(1).unwrap();
    assert_eq!(restored.scenario_rng.logical_state(), source_rng);
    assert_eq!(restored_entity.techno_ctor_random_word(), 0xA55A);
    assert_eq!(restored_entity.building_upgrades().count(), 3);
    assert_eq!(
        restored_entity.building_upgrades().types(),
        &[None, None, Some(upgrade_type)]
    );
    assert_eq!(restored.state_hash(), source_hash);
    assert_eq!(
        restored.building_upgrade_art_names().name(upgrade_type, 0),
        Some("ART_UP")
    );

    let mut changed_word = GameSnapshot::load(&bytes).unwrap().sim;
    let entity = changed_word.substrate.entities.get_mut(1).unwrap();
    entity.set_techno_ctor_random_word_for_test(entity.techno_ctor_random_word() ^ 1);
    assert_ne!(changed_word.state_hash(), source_hash);

    let mut changed_upgrade = restored;
    changed_upgrade
        .substrate
        .entities
        .get_mut(1)
        .unwrap()
        .set_building_upgrades_for_test(2, [None, Some(upgrade_type), None]);
    assert_ne!(changed_upgrade.state_hash(), source_hash);
}

#[test]
fn native_tiberium_cargo_identity_roundtrips_and_changes_miner_and_slave_hashes() {
    use crate::rules::tiberium_type::TiberiumTypeId;
    use crate::sim::miner::{CargoBale, Miner, MinerConfig, MinerKind, ResourceType};
    for slave in [false, true] {
        let mut sim = Simulation::new();
        let mut entity =
            crate::sim::game_entity::GameEntity::test_default(1, "HARV", "Americans", 4, 5);
        let cargo = vec![CargoBale {
            tiberium_type: TiberiumTypeId(2),
            resource_type: ResourceType::Ore,
            value: 25,
        }];
        if slave {
            entity.seldom.slave = crate::sim::slave_manager::SlaveLink::for_test(None, cargo);
        } else {
            let mut miner = Miner::new(MinerKind::War, &MinerConfig::default(), 40);
            miner.set_cargo_for_test(cargo);
            entity.miner = Some(Box::new(miner));
        }
        sim.substrate.entities.insert(entity);
        // Native loading resets Scenario RNG to Seed(0). Keep that
        // independent rule out of this cargo persistence/hash comparison.
        *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let hash = sim.state_hash();
        let bytes = GameSnapshot::save(&sim, 0, 0, "native cargo identity", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        assert_eq!(restored.state_hash(), hash);
        let entity = restored.substrate.entities.get_mut(1).unwrap();
        let mut cargo = if slave {
            entity.seldom.slave.cargo().to_vec()
        } else {
            entity.miner.as_ref().unwrap().cargo().to_vec()
        };
        assert_eq!(cargo[0].tiberium_type, TiberiumTypeId(2));
        // Same presentation/value/amount; only the native slot differs.
        cargo[0].tiberium_type = TiberiumTypeId(0);
        if slave {
            entity.seldom.slave = crate::sim::slave_manager::SlaveLink::for_test(None, cargo);
        } else {
            entity.miner.as_mut().unwrap().set_cargo_for_test(cargo);
        }
        assert_ne!(restored.state_hash(), hash, "slave={slave}");
    }
}

#[test]
fn slave_manager_roundtrips_and_hashes_its_nodes_and_links() {
    use crate::sim::slave_manager::{ManagerState, SlaveManager, SlaveNode, SlaveState};
    let mut sim = Simulation::new();
    let mut parent =
        crate::sim::game_entity::GameEntity::test_default(1, "SMIN", "Americans", 4, 5);
    parent.set_techno_ctor_random_word_for_test(0x1111);
    let slav = sim.intern("SLAV");
    parent.slave_manager = Some(Box::new(SlaveManager::new(
        slav,
        [Some(2), Some(3)],
        500,
        25,
        7,
    )));
    sim.substrate.entities.insert(parent);
    for (stable_id, word) in [(2, 0x2222), (3, 0x3333)] {
        let mut slave =
            crate::sim::game_entity::GameEntity::test_default(stable_id, "SLAV", "Americans", 4, 5);
        slave.set_techno_ctor_random_word_for_test(word);
        // Slave 3 carries one level of gems.
        let cargo = (stable_id == 3)
            .then_some(crate::sim::miner::CargoBale {
                tiberium_type: crate::rules::tiberium_type::TiberiumTypeId(1),
                resource_type: crate::sim::miner::ResourceType::Gem,
                value: 50,
            })
            .into_iter()
            .collect();
        slave.seldom.slave = crate::sim::slave_manager::SlaveLink::for_test(Some(1), cargo);
        sim.substrate.entities.insert(slave);
    }
    *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let source_rng = sim.scenario_rng.logical_state();
    let source_hash = sim.state_hash();

    let bytes = GameSnapshot::save(&sim, 0, 0, "slave-manager", 0);
    let restored = GameSnapshot::load(&bytes).unwrap().sim;
    assert_eq!(restored.scenario_rng.logical_state(), source_rng);
    assert_eq!(
        restored.substrate.entities.get(1).unwrap().slave_manager,
        sim.substrate.entities.get(1).unwrap().slave_manager
    );
    for (stable_id, word) in [(2, 0x2222), (3, 0x3333)] {
        let slave = restored.substrate.entities.get(stable_id).unwrap();
        assert_eq!(slave.techno_ctor_random_word(), word);
        assert_eq!(slave.seldom.slave.owner(), Some(1));
    }
    assert_eq!(
        restored
            .substrate
            .entities
            .get(3)
            .unwrap()
            .seldom
            .slave
            .cargo()
            .len(),
        1
    );
    assert_eq!(restored.state_hash(), source_hash);

    let mut changed_node = GameSnapshot::load(&bytes).unwrap().sim;
    let manager = changed_node
        .substrate
        .entities
        .get_mut(1)
        .unwrap()
        .slave_manager
        .as_mut()
        .unwrap();
    let mut nodes = manager.nodes().to_vec();
    nodes[0].set_state_for_test(SlaveState::Scanning);
    let timer = manager.ai_timer();
    manager.set_for_test(ManagerState::Ready, 0, nodes, timer);
    assert_ne!(changed_node.state_hash(), source_hash);

    let mut changed_order = GameSnapshot::load(&bytes).unwrap().sim;
    let manager = changed_order
        .substrate
        .entities
        .get_mut(1)
        .unwrap()
        .slave_manager
        .as_mut()
        .unwrap();
    let mut nodes: Vec<SlaveNode> = manager.nodes().to_vec();
    nodes.reverse();
    let timer = manager.ai_timer();
    manager.set_for_test(ManagerState::Ready, 0, nodes, timer);
    assert_ne!(changed_order.state_hash(), source_hash);

    let mut changed_owner = restored;
    changed_owner
        .substrate
        .entities
        .get_mut(2)
        .unwrap()
        .seldom
        .slave = crate::sim::slave_manager::SlaveLink::for_test(Some(3), Vec::new());
    assert_ne!(changed_owner.state_hash(), source_hash);
}

#[test]
fn building_anim_slot_restore_rejects_invalid_graph_before_mutation() {
    for case in 0..8 {
        let (mut sim, rules, owner) = crate::sim::building_art::slot_test_fixture();
        let anim = sim
            .set_building_anim_slot(owner, 3, false, false, 0, &rules)
            .unwrap();
        let expected = match case {
            0 | 1 => {
                let target = if case == 0 { 999 } else { owner };
                sim.substrate
                    .entities
                    .get_mut(owner)
                    .unwrap()
                    .building_art
                    .anim_slots_mut_for_test()[3] = Some(target);
                SnapshotRestoreError::MissingBuildingSlotAnim {
                    owner_id: owner,
                    slot: 3,
                    anim_id: target,
                }
            }
            2 => {
                sim.substrate
                    .entities
                    .get_mut(owner)
                    .unwrap()
                    .set_category_for_test(crate::map::entities::EntityCategory::Unit);
                SnapshotRestoreError::InvalidBuildingAnimSlotOwner {
                    owner_id: owner,
                    slot: 3,
                }
            }
            3 => {
                sim.substrate
                    .entities
                    .get_mut(owner)
                    .unwrap()
                    .building_art
                    .anim_slots_mut_for_test()[4] = Some(anim);
                SnapshotRestoreError::DuplicateBuildingSlotAnim {
                    anim_id: anim,
                    first_owner: owner,
                    first_slot: 3,
                    second_owner: owner,
                    second_slot: 4,
                }
            }
            4 => {
                let second = sim.substrate.allocate_stable_id();
                let mut entity = sim.substrate.entities.get(owner).unwrap().clone();
                entity.set_stable_id_for_test(second);
                sim.substrate.entities.insert(entity);
                SnapshotRestoreError::DuplicateBuildingSlotAnim {
                    anim_id: anim,
                    first_owner: owner,
                    first_slot: 3,
                    second_owner: second,
                    second_slot: 3,
                }
            }
            5 => {
                sim.substrate
                    .entities
                    .get_mut(owner)
                    .unwrap()
                    .damage_fires
                    .set_slot_for_test(0, Some(999));
                SnapshotRestoreError::MissingBuildingSlotAnim {
                    owner_id: owner,
                    slot: 21,
                    anim_id: 999,
                }
            }
            6 => {
                sim.substrate
                    .entities
                    .get_mut(owner)
                    .unwrap()
                    .damage_fires
                    .set_slot_for_test(0, Some(anim));
                SnapshotRestoreError::DuplicateBuildingSlotAnim {
                    anim_id: anim,
                    first_owner: owner,
                    first_slot: 3,
                    second_owner: owner,
                    second_slot: 21,
                }
            }
            _ => {
                let entity = sim.substrate.entities.get_mut(owner).unwrap();
                entity.building_art.anim_slots_mut_for_test()[3] = None;
                entity.damage_fires.set_slot_for_test(0, Some(anim));
                entity.set_category_for_test(crate::map::entities::EntityCategory::Unit);
                SnapshotRestoreError::InvalidBuildingAnimSlotOwner {
                    owner_id: owner,
                    slot: 21,
                }
            }
        };
        let before = GameSnapshot::save(&sim, 0, 0, "slot-rejection.map", 0);
        assert_eq!(
            sim.restore_after_snapshot_load(),
            Err(expected),
            "case {case}"
        );
        assert_eq!(
            GameSnapshot::save(&sim, 0, 0, "slot-rejection.map", 0),
            before,
            "restore must reject atomically: {case}"
        );
        assert_eq!(sim.anim(anim).unwrap().building_slot(), Some((owner, 3)));
    }
}

#[test]
fn building_anim_slot_restore_keeps_expired_slots_clear_during_deferred_destroy() {
    let (mut sim, rules, owner) = crate::sim::building_art::slot_test_fixture();
    let anim = sim
        .set_building_anim_slot(owner, 3, false, false, 0, &rules)
        .unwrap();
    assert!(sim.anim(anim).unwrap().owner_entity().is_none());
    sim.destroy_anim(anim, &rules);
    assert!(sim.substrate.pending_delete.contains_object(&anim));
    // Anim UnInit4255B0 broadcasts before deferred physical deletion;
    // Building451B40 clears slot3 immediately but retains Anim+118.
    assert_eq!(
        sim.entities().get(owner).unwrap().building_art.anim_slots()[3],
        None
    );
    assert!(sim.anim(anim).unwrap().building_slot().is_none());
    assert!(sim.anim(anim).unwrap().is_building_anim());
    let bytes = GameSnapshot::save(&sim, 0, 0, "expired-slot.map", 0);
    // Scenario load consumes the saved RNG state and reseeds zero; keep
    // the animation-constructor draw in the saved input, as above.
    *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let hash = sim.state_hash();
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(restored.anim(anim).unwrap().building_slot(), None);
    assert!(restored.anim(anim).unwrap().is_building_anim());
    assert!(restored.substrate.pending_delete.contains_object(&anim));
    assert_eq!(restored.state_hash(), hash);
}

#[test]
fn building_anim_slots_roundtrip_with_runtime_and_reverse_index() {
    let (mut sim, rules, id) = crate::sim::building_art::slot_test_fixture();
    let load_rng = crate::sim::rng::SimRng::new(0).logical_state();
    let anim = sim
        .set_building_anim_slot(id, 3, true, false, 0, &rules)
        .unwrap();
    sim.substrate
        .anims
        .get_mut(anim)
        .unwrap()
        .runtime_mut_for_test()
        .current_frame = 17;
    sim.substrate
        .anims
        .get_mut(anim)
        .unwrap()
        .runtime_mut_for_test()
        .paused = true;
    let building = sim.substrate.entities.get_mut(id).unwrap();
    building.building_art.anim_effect_replay_mut_for_test()[16] = true;
    building
        .building
        .building_last_operational
        .set_for_test(true);
    building.building_art.set_stuff_enabled_for_test(false);
    building
        .building_art
        .storage_mut_for_test()
        .set_amounts_for_test([
            crate::util::native_x87::NativeF32Bits::from_bits(0x7fc01234),
            crate::util::native_x87::NativeF32Bits::NEGATIVE_ZERO,
            crate::util::native_x87::NativeF32Bits::ONE,
            crate::util::native_x87::NativeF32Bits::from_bits(0xbf800000),
        ]);
    building
        .building_art
        .storage_mut_for_test()
        .set_refinery_tier_for_test(-17);
    assert_ne!(sim.scenario_rng.logical_state(), load_rng);
    let bytes = GameSnapshot::save(&sim, 1, 2, "building-anim.map", 0);
    // Native Scenario load consumes the saved RNG bytes, then reseeds zero.
    // The RandomRate constructor draw remains observable before saving;
    // compare all saved slot/runtime state against the native load state.
    *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let expected = sim.state_hash();
    assert_eq!(
        GameSnapshot::read_header(&bytes).unwrap().layout,
        snapshot_layout()
    );
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(restored.scenario_rng.logical_state(), load_rng);
    assert_eq!(
        restored
            .entities()
            .get(id)
            .unwrap()
            .building_art
            .anim_slots()[3],
        Some(anim)
    );
    assert!(
        restored
            .entities()
            .get(id)
            .unwrap()
            .building_art
            .damage_state_active()
    );
    assert_eq!(restored.anim(anim).unwrap().runtime().current_frame, 17);
    assert!(restored.anim(anim).unwrap().runtime().paused);
    assert!(
        restored
            .entities()
            .get(id)
            .unwrap()
            .building_art
            .anim_effect_replay()[16]
    );
    assert!(
        restored
            .entities()
            .get(id)
            .unwrap()
            .building
            .building_last_operational
            .is_operational()
    );
    assert!(
        !restored
            .entities()
            .get(id)
            .unwrap()
            .building_art
            .stuff_enabled()
    );
    assert_eq!(
        restored.entities().get(id).unwrap().building_art.storage(),
        sim.entities().get(id).unwrap().building_art.storage()
    );
    assert_eq!(restored.anim(anim).unwrap().building_slot(), Some((id, 3)));
    assert_eq!(restored.state_hash(), expected);
}

#[test]
fn gsi_13_06_body_frame_counter_roundtrips_and_changes_hash() {
    use crate::map::entities::EntityCategory;
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::combat::Health;
    use crate::sim::components::DriveCoord;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::ShipLocomotionRuntime;
    use crate::sim::movement::locomotor::LocomotorState;
    use crate::util::fixed_math::SIM_ONE;

    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Soviet");
    let type_ref = sim.interner.intern("DRON");
    sim.substrate
        .entities
        .insert(GameEntity::new_at_frame_zero_for_test(
            1,
            5,
            5,
            0,
            0,
            owner,
            Health::new(100),
            type_ref,
            EntityCategory::Unit,
            0,
            5,
            false,
        ));
    *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let zero_counter_hash = sim.state_hash();
    sim.substrate
        .entities
        .get_mut(1)
        .expect("Terror Drone")
        .body_frame_counter
        .set_for_test(u32::MAX);
    let populated_counter_hash = sim.state_hash();
    assert_ne!(populated_counter_hash, zero_counter_hash);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .unit_firing_counter
        .set_for_test(7);
    assert_ne!(sim.state_hash(), populated_counter_hash);

    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .foot_speed
        .set_speed_fraction_native_bits(0x3fc9_9999_9999_999a);
    let fraction_hash = sim.state_hash();
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .foot_speed
        .set_speed_fraction_native_bits(0x3fc9_9999_9999_999b);
    assert_ne!(
        sim.state_hash(),
        fraction_hash,
        "adjacent doubles with the same fixed projection must hash differently"
    );
    let ship_head = DriveCoord::cell(6, 5, 0);
    let entity = sim.substrate.entities.get_mut(1).expect("SHP unit");
    entity
        .foot_speed
        .set_speed_fraction_native_bits(0x3fc9_9999_9999_999a);
    // This serialization fixture supplies a Ship instance explicitly.
    entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Ship));
    entity
        .locomotor
        .as_mut()
        .unwrap()
        .install_ship_state_for_test(Some(
            ShipLocomotionRuntime::default()
                .with_destination_for_test(Some(ship_head))
                .with_head_to_for_test(Some(ship_head))
                .with_target_speed_fraction_for_test(SIM_ONE),
        ));
    entity
        .locomotor
        .as_mut()
        .unwrap()
        .store_track_target_fraction_native(
            crate::sim::movement::track_process::TrackFamily::Ship,
            crate::util::native_x87::NativeF64Bits::from_bits(0x3ff3_3333_3333_3334),
        );
    let populated_shp_state_hash = sim.state_hash();
    assert_ne!(populated_shp_state_hash, populated_counter_hash);

    let bytes = GameSnapshot::save(&sim, 1, 2, "counter.map", 0);
    let restored = GameSnapshot::load(&bytes)
        .expect("v82 body-counter snapshot")
        .sim;
    assert_eq!(
        restored
            .substrate
            .entities
            .get(1)
            .expect("restored Terror Drone")
            .body_frame_counter
            .get(),
        u32::MAX
    );
    assert_eq!(
        restored
            .entities()
            .get(1)
            .unwrap()
            .unit_firing_counter
            .get(),
        7
    );
    let restored_ship = restored
        .substrate
        .entities
        .get(1)
        .expect("restored SHP unit")
        .locomotor
        .as_ref()
        .and_then(|loco| loco.selected_ship_runtime())
        .and_then(|runtime| runtime.retained())
        .expect("restored Ship runtime");
    assert_eq!(restored_ship.destination(), Some(ship_head));
    assert_eq!(restored_ship.head_to(), Some(ship_head));
    assert_eq!(
        restored
            .entities()
            .get(1)
            .unwrap()
            .locomotor
            .as_ref()
            .unwrap()
            .track_target_fraction_native(crate::sim::movement::track_process::TrackFamily::Ship)
            .unwrap()
            .bits(),
        0x3ff3_3333_3333_3334
    );
    let restored_owner_speed = &restored.substrate.entities.get(1).unwrap().foot_speed;
    assert_eq!(
        restored_owner_speed.native_applied_fraction().bits(),
        0x3fc9_9999_9999_999a
    );
    assert_eq!(restored.state_hash(), populated_shp_state_hash);
}

#[test]
fn gsi_05_10_pending_building_fire_roundtrips_and_changes_hash() {
    use crate::map::entities::EntityCategory;
    use crate::sim::combat::Health;
    use crate::sim::combat::combat_weapon::WeaponSlot;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::projectile::ProjectileCoord;
    use crate::sim::world::{DelayedFire, PendingBuildingFire};

    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Soviet");
    let type_ref = sim.interner.intern("NATSLA");
    for id in [1, 2] {
        let entity = GameEntity::new_at_frame_zero_for_test(
            id,
            5,
            5,
            0,
            0,
            owner,
            Health::new(600),
            type_ref,
            EntityCategory::Structure,
            0,
            8,
            false,
        );
        sim.substrate.entities.insert(entity);
    }
    // Full snapshot load resets Scenario RNG to Seed0. Compare the
    // authoritative delayed-fire state on that same post-load cursor.
    *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let without_latch = sim.state_hash();
    let shot = PendingBuildingFire::new(17, DelayedFire::Weapon(WeaponSlot::Secondary));
    sim.substrate
        .entities
        .get_mut(1)
        .expect("Tesla Coil")
        .building
        .pending_building_fire
        .set_for_test(Some(shot));
    let with_latch = sim.state_hash();
    assert_ne!(with_latch, without_latch);
    let beam = PendingBuildingFire::new(
        17,
        DelayedFire::SupportBeam {
            to: ProjectileCoord::new(1280, 1408, 378),
        },
    );
    let supporter = sim.substrate.entities.get_mut(2).expect("second tower");
    supporter
        .building
        .pending_building_fire
        .set_for_test(Some(beam));
    supporter.building.prism_support_count.set_for_test(3);
    let with_beam = sim.state_hash();
    assert_ne!(with_beam, with_latch);

    let bytes = GameSnapshot::save(&sim, 1, 2, "delay.map", 0);
    let restored = GameSnapshot::load(&bytes)
        .expect("delayed-fire snapshot")
        .sim;
    let entities = &restored.substrate.entities;
    assert_eq!(
        entities
            .get(1)
            .unwrap()
            .building
            .pending_building_fire
            .pending(),
        Some(shot)
    );
    assert_eq!(
        entities
            .get(2)
            .unwrap()
            .building
            .pending_building_fire
            .pending(),
        Some(beam)
    );
    assert_eq!(
        entities
            .get(2)
            .unwrap()
            .building
            .prism_support_count
            .count(),
        3
    );
    assert_eq!(restored.state_hash(), with_beam);
}

#[test]
fn gsi_04_15_active_tube_detachment_and_exact_z_roundtrip_hash() {
    use crate::map::tube_facts::TubeId;
    use crate::sim::components::DriveCoord;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::tube_movement::LowBridgeTubeMovementState;

    let mut sim = Simulation::new();
    let entity_id = sim.substrate.allocate_stable_id();
    let mut entity = GameEntity::test_default(entity_id, "MTNK", "Americans", 5, 7);
    entity.lifecycle.set_in_limbo_for_test(false);
    entity.lifecycle.set_cell_marked_for_test(false);
    entity.seldom.in_logic_vector.set_for_test(true);
    entity.position.set_z_for_test(3);
    entity.position.set_exact_z_leptons_for_test(Some(-37));
    entity.low_bridge_tube_state = Some(LowBridgeTubeMovementState::for_test(
        TubeId(9),
        2,
        DriveCoord {
            x: 1_536,
            y: 2_048,
            z: -19,
        },
    ));
    sim.substrate.entities.insert(entity);
    sim.substrate
        .logic
        .try_push(entity_id, crate::sim::world::ObjectKind::Entity)
        .expect("active TubeMovement fixture enters LogicClass order");
    // Native in-scenario load restarts Scenario RNG from Seed0; isolate
    // TubeMovement persistence on that same post-load cursor.
    *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let expected_hash = sim.state_hash();

    let bytes = GameSnapshot::save(&sim, 0, 0, "gsi_04_15_tube", 0);
    assert_eq!(
        GameSnapshot::read_header(&bytes).unwrap().layout,
        super::snapshot_layout()
    );
    let mut restored = GameSnapshot::load(&bytes)
        .expect("current tube snapshot")
        .sim;
    restored
        .restore_after_snapshot_load()
        .expect("active detached tube object restores");

    let restored_entity = restored
        .substrate
        .entities
        .get(entity_id)
        .expect("tube mover restored");
    assert_eq!(restored_entity.position.z(), 3);
    assert_eq!(restored_entity.position.exact_z_leptons(), Some(-37));
    assert_eq!(
        restored_entity.low_bridge_tube_state,
        Some(LowBridgeTubeMovementState::for_test(
            TubeId(9),
            2,
            DriveCoord {
                x: 1_536,
                y: 2_048,
                z: -19,
            }
        ))
    );
    assert!(!restored_entity.lifecycle.cell_marked());
    assert!(
        !restored
            .substrate
            .occupancy
            .contains_entity(5, 7, entity_id)
    );
    assert_eq!(restored.substrate.raw_cell_occupation.ground_bits(5, 7), 0);
    assert_eq!(restored.state_hash(), expected_hash);
}

#[test]
fn gsi_04_07_wall_sell_pending_command_house_mode_roundtrip_and_hash() {
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Receiver");
    let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, false, 0, 10);
    house.set_player_control_for_test(true);
    sim.houses.register(house);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.queue_command(crate::sim::command::CommandEnvelope::new(
        owner,
        17,
        crate::sim::command::Command::SellWallAtCell { x: -3, y: 9 },
    ));
    // Native in-scenario load restarts Scenario RNG from Seed0; isolate
    // pending-command/house-mode persistence on that post-load cursor.
    *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let expected_hash = sim.state_hash();
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_player_control_for_test(false);
    assert_ne!(sim.state_hash(), expected_hash);
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_player_control_for_test(true);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(false));
    assert_ne!(sim.state_hash(), expected_hash);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));

    let bytes = GameSnapshot::save(&sim, 1, 2, "wall.map", 0);
    let restored = GameSnapshot::load(&bytes)
        .expect("v60 wall-sale snapshot")
        .sim;
    assert_eq!(
        restored.pending_command_snapshot(),
        sim.pending_command_snapshot()
    );
    assert!(restored.houses.get(&owner).unwrap().player_control());
    assert!(restored.session.game_mode_nonzero());
    assert_eq!(restored.state_hash(), expected_hash);
}

#[test]
fn gsi_04_07_damage_v60_air_spatial_armor_berserk_hostile_hit_iq_smoke_capture_anger_and_delay_roundtrip_hash()
 {
    let mut sim = Simulation::new();
    let entity_id = sim.substrate.allocate_stable_id();
    let mut aircraft =
        crate::sim::game_entity::GameEntity::test_default(entity_id, "ORCA", "AMERICANS", 11, 7);
    aircraft = aircraft.with_air_spatial_membership_for_test(Some(143), 91);
    aircraft
        .seldom
        .crate_multipliers
        .set_armor_multiplier_for_test(crate::util::native_x87::NativeF64Bits::from_bits(
            1.5_f64.to_bits(),
        ));
    aircraft.berserk.set_active_for_test(true);
    aircraft.berserk.set_timer_for_test(-17);
    aircraft
        .seldom
        .ai_sell_repair
        .set_was_attacked_by_enemy_for_test(true);
    aircraft.seldom.damage_smoke_system_id.set_for_test(Some(2));
    aircraft.capture_manager = Some(Box::new(
        crate::sim::capture_manager::CaptureManagerState::with_victims_for_test(3, false, &[3, 4]),
    ));
    aircraft.building.pending_c4_detonation.set_for_test(Some(
        crate::sim::c4_plant::PendingC4Detonation::new(
            crate::sim::timer::CdTimer::started(11, 35),
            Some(3),
        ),
    ));
    sim.substrate.entities.insert(aircraft);
    sim.substrate
        .entities
        .insert(crate::sim::game_entity::GameEntity::test_default(
            3,
            "E1",
            "AMERICANS",
            12,
            7,
        ));
    sim.substrate
        .entities
        .insert(crate::sim::game_entity::GameEntity::test_default(
            4,
            "E1",
            "AMERICANS",
            13,
            7,
        ));
    sim.substrate.particle_systems.insert({
        let mut system = crate::sim::particles::ParticleSystem::test_fixture(
            2,
            crate::rules::particle_system_type::ParticleSystemTypeId(0),
            glam::IVec3::ZERO,
        );
        system.set_offset_for_test(glam::IVec3::ZERO);
        system.set_spawn_timer_for_test(crate::util::native_x87::NativeF32Bits::ONE);
        system.set_owner_entity_for_test(Some(entity_id));
        system.set_target_coords_for_test(glam::IVec3::ZERO);
        system
    });
    sim.set_next_stable_object_id_for_test(5);
    let owner = sim.interner.intern("ComputerIQ");
    let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, false, 0, 51);
    house.set_current_iq_for_test(2);
    let threat_peer = sim.interner.intern("ThreatPeer");
    house.grudge_scores_mut_for_test().insert(threat_peer, 350);
    house.set_enemy_house_for_test(Some(threat_peer));
    sim.houses.register(house);
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            threat_peer,
            1,
            None,
            false,
            0,
            51,
        ));
    // Native in-scenario load restarts Scenario RNG from Seed0; isolate
    // damage-authority persistence on that same post-load cursor.
    *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let expected_hash = sim.state_hash();

    {
        let entity = sim.substrate.entities.get_mut(entity_id).unwrap();
        *entity = entity
            .clone()
            .with_air_spatial_membership_for_test(Some(143), 92);
    }
    assert_ne!(sim.state_hash(), expected_hash, "vector order is hashed");
    {
        let entity = sim.substrate.entities.get_mut(entity_id).unwrap();
        *entity = entity
            .clone()
            .with_air_spatial_membership_for_test(Some(144), 91);
    }
    assert_ne!(sim.state_hash(), expected_hash, "bucket identity is hashed");
    {
        let entity = sim.substrate.entities.get_mut(entity_id).unwrap();
        *entity = entity
            .clone()
            .with_air_spatial_membership_for_test(Some(143), 91);
    }
    assert_eq!(sim.state_hash(), expected_hash);
    sim.substrate
        .entities
        .get_mut(entity_id)
        .unwrap()
        .seldom
        .crate_multipliers
        .set_armor_multiplier_for_test(crate::util::native_x87::NativeF64Bits::ONE);
    assert_ne!(sim.state_hash(), expected_hash, "instance armor is hashed");
    sim.substrate
        .entities
        .get_mut(entity_id)
        .unwrap()
        .seldom
        .crate_multipliers
        .set_armor_multiplier_for_test(crate::util::native_x87::NativeF64Bits::from_bits(
            1.5_f64.to_bits(),
        ));
    sim.substrate
        .entities
        .get_mut(entity_id)
        .unwrap()
        .berserk
        .set_timer_for_test(-18);
    assert_ne!(sim.state_hash(), expected_hash, "berserk timer is hashed");
    {
        let entity = sim.substrate.entities.get_mut(entity_id).unwrap();
        entity.berserk.set_timer_for_test(-17);
        entity.berserk.set_active_for_test(false);
    }
    assert_ne!(
        sim.state_hash(),
        expected_hash,
        "berserk active byte is hashed"
    );
    sim.substrate
        .entities
        .get_mut(entity_id)
        .unwrap()
        .berserk
        .set_active_for_test(true);
    sim.substrate
        .entities
        .get_mut(entity_id)
        .unwrap()
        .seldom
        .ai_sell_repair
        .set_was_attacked_by_enemy_for_test(false);
    assert_ne!(
        sim.state_hash(),
        expected_hash,
        "hostile-hit latch is hashed"
    );
    sim.substrate
        .entities
        .get_mut(entity_id)
        .unwrap()
        .seldom
        .ai_sell_repair
        .set_was_attacked_by_enemy_for_test(true);
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_current_iq_for_test(1);
    assert_ne!(sim.state_hash(), expected_hash, "CurrentIQ is hashed");
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_current_iq_for_test(2);
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .grudge_scores_mut_for_test()
        .insert(threat_peer, 351);
    assert_ne!(sim.state_hash(), expected_hash, "anger score is hashed");
    {
        let house = sim.houses.get_mut(&owner).unwrap();
        house.grudge_scores_mut_for_test().insert(threat_peer, 350);
        house.set_enemy_house_for_test(None);
    }
    assert_ne!(
        sim.state_hash(),
        expected_hash,
        "selected enemy identity is hashed"
    );
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_enemy_house_for_test(Some(threat_peer));
    sim.substrate
        .entities
        .get_mut(entity_id)
        .unwrap()
        .seldom
        .damage_smoke_system_id
        .set_for_test(None);
    assert_ne!(
        sim.state_hash(),
        expected_hash,
        "damage-Smoke identity is hashed"
    );
    sim.substrate
        .entities
        .get_mut(entity_id)
        .unwrap()
        .seldom
        .damage_smoke_system_id
        .set_for_test(Some(2));
    sim.substrate
        .entities
        .get_mut(entity_id)
        .unwrap()
        .capture_manager
        .as_mut()
        .unwrap()
        .reverse_nodes_for_test();
    assert_ne!(sim.state_hash(), expected_hash, "MCNode order is hashed");
    sim.substrate
        .entities
        .get_mut(entity_id)
        .unwrap()
        .capture_manager
        .as_mut()
        .unwrap()
        .reverse_nodes_for_test();

    let bytes = GameSnapshot::save(&sim, 0, 0, "gsi_04_07_air_spatial", 0);
    assert_eq!(
        GameSnapshot::read_header(&bytes).unwrap().layout,
        super::snapshot_layout()
    );
    let mut restored = GameSnapshot::load(&bytes).expect("v60 snapshot").sim;
    restored
        .restore_after_snapshot_load()
        .expect("damage-Smoke pointer resolves through ParticleSystemStore");
    let entity = restored.substrate.entities.get(entity_id).unwrap();
    assert_eq!(entity.air_spatial_bucket(), Some(143));
    assert_eq!(entity.air_spatial_enter_order(), 91);
    assert_eq!(entity.armor_multiplier().bits(), 1.5_f64.to_bits());
    assert!(entity.berserk.is_active());
    assert_eq!(entity.berserk.timer(), -17);
    assert!(entity.seldom.ai_sell_repair.was_attacked_by_enemy());
    assert_eq!(entity.seldom.damage_smoke_system_id.system(), Some(2));
    assert_eq!(
        entity.building.pending_c4_detonation.pending(),
        Some(crate::sim::c4_plant::PendingC4Detonation::new(
            crate::sim::timer::CdTimer::started(11, 35),
            Some(3)
        ))
    );
    assert_eq!(
        entity
            .capture_manager
            .as_ref()
            .map(|manager| manager.victims().collect::<Vec<_>>()),
        Some(vec![3, 4])
    );
    assert_eq!(restored.houses.get(&owner).unwrap().current_iq(), 2);
    assert_eq!(
        restored.houses[&owner].grudge_scores().get(&threat_peer),
        Some(&350)
    );
    assert_eq!(restored.houses[&owner].enemy_house(), Some(threat_peer));
    assert_eq!(restored.state_hash(), expected_hash);
}

#[test]
fn gsi_04_05_hidden_snapshot_and_hash_preserve_counter_and_exit_profile() {
    use crate::map::entities::EntityCategory;
    use crate::sim::game_entity::GameEntity;

    let mut sim = Simulation::new();
    sim.session.set_map_width_for_test(32);
    sim.session.set_map_height_for_test(32);
    let entity_id = sim.substrate.allocate_stable_id();
    let mut building = GameEntity::test_default_of_category(
        entity_id,
        "GAREFN",
        "AMERICANS",
        10,
        10,
        EntityCategory::Structure,
    );
    building
        .seldom
        .type_facts
        .set_foundation_for_test("4x3".to_string().into());
    let mut profile = crate::rules::object_type::BuildingHiddenOccupancyProfile::default();
    profile.add_occupy[0] = Some((-1, 0));
    profile.add_occupy[1] = Some((-1, -1));
    profile.remove_occupy[0] = Some((3, 1));
    building
        .seldom
        .type_facts
        .set_building_hidden_occupancy_for_test(Some(profile));
    sim.substrate.entities.insert(building);
    sim.add_entity_occupancy(entity_id);

    // Native in-scenario load restarts Scenario RNG from Seed0; isolate
    // hidden-occupation persistence on that same post-load cursor.
    *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let expected_hash = sim.state_hash();
    sim.substrate.hidden_occupation.exit_building(
        (10, 10),
        &"4x3".into(),
        profile,
        Some((32, 32)),
        false,
    );
    assert_ne!(sim.state_hash(), expected_hash);
    sim.substrate.hidden_occupation.enter_building(
        (10, 10),
        &"4x3".into(),
        profile,
        Some((32, 32)),
        false,
    );
    let mut stored = profile;
    stored.occupy_height = 4;
    sim.substrate
        .entities
        .get_mut(entity_id)
        .unwrap()
        .seldom
        .type_facts
        .set_building_hidden_occupancy_for_test(Some(stored));
    assert_ne!(sim.state_hash(), expected_hash);
    stored.occupy_height = 2;
    sim.substrate
        .entities
        .get_mut(entity_id)
        .unwrap()
        .seldom
        .type_facts
        .set_building_hidden_occupancy_for_test(Some(stored));
    assert_eq!(sim.state_hash(), expected_hash);

    let bytes = GameSnapshot::save(&sim, 0, 0, "gsi_04_05_hidden", 0);
    let mut restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
    restored
        .restore_after_snapshot_load()
        .expect("restore skipped cell-list caches");

    assert_eq!(restored.substrate.hidden_occupation.count(9, 9), 1);
    assert_eq!(restored.substrate.hidden_occupation.count(13, 11), 0);
    assert_eq!(
        restored
            .substrate
            .entities
            .get(entity_id)
            .unwrap()
            .seldom
            .type_facts
            .building_hidden_occupancy(),
        Some(profile)
    );
    assert_eq!(restored.state_hash(), expected_hash);
}

#[test]
fn gsi_04_05_reservation_roundtrip_preserves_real_and_house_state_but_clears_dummy() {
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("AMERICANS");
    let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 10);
    house.base_reservation_mut().update_bounds(3, 4, 5, 6);
    house
        .base_reservation_mut()
        .append_perimeter_cell_if_absent(u32::from(3u16) | (u32::from(4u16) << 16));
    sim.houses.register(house);
    let entity_id = sim.substrate.allocate_stable_id();
    let mut building = crate::sim::game_entity::GameEntity::test_default_of_category(
        entity_id,
        "GAPOWR",
        "AMERICANS",
        3,
        4,
        crate::map::entities::EntityCategory::Structure,
    );
    building.set_owner_for_test(owner);
    building
        .seldom
        .type_facts
        .set_base_reservation_spacing_for_test(Some(-3));
    sim.substrate.entities.insert(building);
    let mut reservation_terrain = flat_terrain(4, 5);
    reservation_terrain.test_set_native_allocated_cells(&[(3, 4)]);
    *sim.resolved_terrain = Some(reservation_terrain);
    sim.substrate
        .base_reservations
        .reserve(sim.resolved_terrain.as_ref(), 3, 4, 0);
    sim.substrate
        .base_reservations
        .reserve(sim.resolved_terrain.as_ref(), 1, 0, 1);
    assert_eq!(
        sim.substrate
            .base_reservations
            .raw_mask(sim.resolved_terrain.as_ref(), 2, 0),
        1 << 1,
        "distinct valid-linear null slots share the dummy before save"
    );
    // Native in-scenario load restarts Scenario RNG from Seed0; isolate
    // base-reservation persistence on that same post-load cursor.
    *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let expected_hash = sim.state_hash();

    sim.substrate
        .base_reservations
        .clear(sim.resolved_terrain.as_ref(), 3, 4, 0);
    assert_ne!(sim.state_hash(), expected_hash, "real mask is hashed");
    sim.substrate
        .base_reservations
        .reserve(sim.resolved_terrain.as_ref(), 3, 4, 0);
    sim.substrate
        .base_reservations
        .clear(sim.resolved_terrain.as_ref(), 2, 0, 1);
    assert_ne!(
        sim.state_hash(),
        expected_hash,
        "valid-linear native-unallocated shared dummy mask is hashed"
    );
    sim.substrate
        .base_reservations
        .reserve(sim.resolved_terrain.as_ref(), 1, 0, 1);
    sim.substrate
        .entities
        .get_mut(entity_id)
        .unwrap()
        .seldom
        .type_facts
        .set_base_reservation_spacing_for_test(Some(9));
    assert_ne!(
        sim.state_hash(),
        expected_hash,
        "entity writer profile is hashed"
    );
    sim.substrate
        .entities
        .get_mut(entity_id)
        .unwrap()
        .seldom
        .type_facts
        .set_base_reservation_spacing_for_test(Some(-3));
    assert_eq!(sim.state_hash(), expected_hash);

    let bytes = GameSnapshot::save(&sim, 0, 0, "gsi_04_05_reservation", 0);
    assert_eq!(
        GameSnapshot::read_header(&bytes).unwrap().layout,
        super::snapshot_layout()
    );
    sim.substrate
        .base_reservations
        .clear(sim.resolved_terrain.as_ref(), 2, 0, 1);
    let expected_loaded_hash = sim.state_hash();

    let mut restored = GameSnapshot::load(&bytes)
        .expect("reservation snapshot")
        .sim;
    restored
        .restore_after_snapshot_load()
        .expect("restore transient caches without rebuilding reservations");
    assert_eq!(restored.substrate.base_reservations.raw_mask(None, 3, 4), 1);
    assert_eq!(restored.substrate.base_reservations.dummy_mask(), 0);
    let restored_house = restored.houses.get(&owner).expect("restored owner");
    assert_eq!(restored_house.base_reservation().bounds(), (3, 4, 5, 6));
    assert_eq!(
        restored_house.base_reservation().perimeter_cells(),
        &[u32::from(3u16) | (u32::from(4u16) << 16)]
    );
    assert_eq!(
        restored
            .substrate
            .entities
            .get(entity_id)
            .unwrap()
            .seldom
            .type_facts
            .base_reservation_spacing(),
        Some(-3)
    );
    assert_eq!(restored.state_hash(), expected_loaded_hash);
}

#[test]
fn gsi_04_05_v40_roundtrip_restores_drive_runtime_and_cleared_foot_occupation() {
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::DriveLocomotionRuntime;
    use crate::sim::movement::locomotor::LocomotorState;

    let mut sim = Simulation::new();
    let owner = sim.interner.intern("AMERICANS");
    let type_ref = sim.interner.intern("MTNK");
    let entity_id = sim.substrate.allocate_stable_id();
    let mut entity = GameEntity::test_default(entity_id, "MTNK", "AMERICANS", 2, 2);
    entity.set_owner_for_test(owner);
    entity.set_type_ref_for_test(type_ref);
    entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    sim.substrate.entities.insert(entity);
    sim.add_entity_occupancy(entity_id);

    sim.substrate
        .entities
        .get_mut(entity_id)
        .expect("Drive unit")
        .locomotor
        .as_mut()
        .unwrap()
        .install_drive_state_for_test(Some(DriveLocomotionRuntime::default()));
    sim.substrate
        .entities
        .get_mut(entity_id)
        .unwrap()
        .set_foot_occupation_enabled(false);
    // Native in-scenario load restarts Scenario RNG from Seed0; isolate
    // Drive and Foot enable persistence on that same post-load cursor.
    *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let expected_hash = sim.state_hash();

    let bytes = GameSnapshot::save(&sim, 0, 0, "gsi_04_05", 0);
    assert_eq!(
        GameSnapshot::read_header(&bytes)
            .expect("current snapshot header")
            .layout,
        super::snapshot_layout()
    );

    let mut restored_a = GameSnapshot::load(&bytes).expect("current snapshot").sim;
    let mut restored_b = GameSnapshot::load(&bytes).expect("current snapshot").sim;
    restored_a
        .restore_after_snapshot_load()
        .expect("first deterministic rebuild");
    restored_b
        .restore_after_snapshot_load()
        .expect("second deterministic rebuild");

    for restored in [&restored_a, &restored_b] {
        assert!(
            restored
                .substrate
                .entities
                .get(entity_id)
                .expect("restored Drive unit")
                .locomotor
                .as_ref()
                .and_then(|loco| loco.selected_drive_runtime())
                .and_then(|runtime| runtime.retained())
                .is_some(),
            "restored Drive runtime"
        );
        assert!(
            !restored
                .substrate
                .entities
                .get(entity_id)
                .unwrap()
                .foot_occupation_enabled()
        );
        assert_eq!(restored.state_hash(), expected_hash);
    }
}

#[test]
fn gsi_04_07_v40_roundtrip_restores_wall_owner() {
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("AMERICANS");
    let mut overlays = crate::sim::overlay_grid::OverlayGrid::new(8, 8);
    overlays.place_owned_wall(3, 4, 2, 0x1A, owner);
    *sim.overlay_grid = Some(overlays);
    // Native in-scenario load restarts Scenario RNG from Seed0; isolate
    // wall-owner persistence on that same post-load cursor.
    *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let expected_hash = sim.state_hash();
    let mut unowned = Simulation::new();
    *unowned.scenario_rng = crate::sim::rng::SimRng::new(0);
    let _ = unowned.interner.intern("AMERICANS");
    let mut unowned_overlays = crate::sim::overlay_grid::OverlayGrid::new(8, 8);
    unowned_overlays.place_overlay(3, 4, 2, 0x1A);
    *unowned.overlay_grid = Some(unowned_overlays);
    assert_ne!(
        unowned.state_hash(),
        expected_hash,
        "wall ownership participates in deterministic state"
    );

    let bytes = GameSnapshot::save(&sim, 0, 0, "gsi_04_07", 0);
    assert_eq!(
        GameSnapshot::read_header(&bytes)
            .expect("current snapshot header")
            .layout,
        super::snapshot_layout()
    );
    let restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
    let cell = restored
        .overlay_grid
        .as_ref()
        .expect("overlay grid")
        .cell(3, 4);
    assert_eq!(cell.overlay_id, Some(2));
    assert_eq!(cell.overlay_data, 0x1A);
    assert_eq!(cell.wall_owner, Some(owner));
    assert_eq!(restored.state_hash(), expected_hash);
}

#[test]
fn exact_mission_schema_round_trips_raw_ids_leaves_archives_and_locomotors() {
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::combat::TargetKind;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::mission::state::MissionTestFixture;
    use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionLeafState};
    use crate::sim::movement::locomotor::LocomotorState;

    let mut leaves = [
        MissionLeafState::unit_raw_for_test(1, 2, 3, 4),
        MissionLeafState::infantry_raw_for_test(5, 41),
        MissionLeafState::aircraft_raw_for_test(6, 7),
        MissionLeafState::building_raw_for_test(8),
        MissionLeafState::unit_raw_for_test(9, 10, 11, 12),
        MissionLeafState::infantry_raw_for_test(13, -1),
    ];
    leaves[1].set_infantry_water_state(false);
    leaves[5].set_infantry_water_state(true);

    let mut sim = Simulation::new();
    for (index, &leaf) in leaves.iter().enumerate() {
        let id = index as u64 + 1;
        let mut entity = GameEntity::test_default(id, "MTNK", "Americans", 5, 5);
        entity.mission_leaf = leaf;
        entity
            .seldom
            .suspended_attack_target
            .set_for_test(Some(if index & 1 == 0 {
                TargetKind::Entity(100 + id)
            } else {
                TargetKind::Cell(index as u16, (index + 1) as u16)
            }));
        entity.set_falling_down_for_test(index & 1 == 0);
        entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
        if index == 0 {
            entity.mission.apply_test_fixture(MissionTestFixture {
                current: MissionId::from_raw(i32::MIN),
                suspended: MissionId::from_raw(0x1234_5678),
                queued: MissionId::from_raw(i32::MAX),
                movement_bypass_latch: 0xa5,
                handler_state: 0x1122_3344,
                mission_start_frame: 0x5566_7788,
                ai_counter: 0x99aa_bbcc,
                dispatch_timer: MissionDispatchTimer::from_raw(-17, -29),
            });
        }
        sim.substrate.entities.insert(entity);
    }

    let bytes = GameSnapshot::save(&sim, 1, 2, "mission-schema", 3);
    let loaded = GameSnapshot::load(&bytes).expect("load exact Mission schema");

    for (index, &leaf) in leaves.iter().enumerate() {
        let entity = loaded
            .sim
            .substrate
            .entities
            .get(index as u64 + 1)
            .expect("restored Mission fixture");
        let id = index as u64 + 1;
        let expected_suspended_target = Some(if index & 1 == 0 {
            TargetKind::Entity(100 + id)
        } else {
            TargetKind::Cell(index as u16, (index + 1) as u16)
        });
        assert_eq!(entity.mission_leaf, leaf);
        assert_eq!(
            entity.seldom.suspended_attack_target.target(),
            expected_suspended_target,
            "suspended TargetKind variant and payload must round-trip"
        );
        assert_eq!(entity.is_falling_down(), index & 1 == 0);
    }

    let first = loaded.sim.substrate.entities.get(1).unwrap();
    assert_eq!(first.mission.current(), MissionId::from_raw(i32::MIN));
    assert_eq!(first.mission.suspended(), MissionId::from_raw(0x1234_5678));
    assert_eq!(first.mission.queued(), MissionId::from_raw(i32::MAX));
    assert_eq!(
        first.mission.dispatch_timer(),
        MissionDispatchTimer::from_raw(-17, -29)
    );
}

#[test]
fn constructor_owner_survives_transfers_and_snapshot_restore() {
    use crate::map::entities::EntityCategory;
    use crate::sim::game_entity::GameEntity;

    let mut sim = Simulation::new();
    let initial = sim.interner.intern("Americans");
    let current = sim.interner.intern("Russians");
    let type_ref = sim.interner.intern("GAPOWR");
    let mut entity = GameEntity::test_default_of_category(
        1,
        "GAPOWR",
        "Americans",
        5,
        5,
        EntityCategory::Structure,
    );
    entity.set_owner_for_test(initial);
    entity.set_type_ref_for_test(type_ref);
    sim.substrate.entities.insert(entity);
    sim.substrate.entities.change_owner(1, current);
    // Match the scenario RNG's post-load seed reset for this state-only
    // snapshot test. Physical resumed capture is tested separately.
    *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let expected_hash = sim.state_hash();
    let bytes = GameSnapshot::save(&sim, 0, 0, "captured-owner", 0);
    let mut loaded = GameSnapshot::load(&bytes).expect("load constructor owner");
    let restored = loaded.sim.substrate.entities.get(1).unwrap();
    assert_eq!(restored.owner(), current);
    assert_eq!(restored.initial_owner(), initial);
    assert_eq!(loaded.sim.state_hash(), expected_hash);
    loaded.sim.substrate.entities.change_owner(1, initial);
    let recaptured = loaded.sim.substrate.entities.get(1).unwrap();
    assert_eq!(recaptured.owner(), initial);
    assert_eq!(recaptured.initial_owner(), initial);
}

#[test]
fn entity_construction_frame_round_trips_as_dispatch_start() {
    use crate::map::entities::EntityCategory;
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;

    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Americans");
    let type_ref = sim.interner.intern("MTNK");
    let entity = GameEntity::new_at_frame_for_test(
        1,
        5,
        5,
        0,
        0,
        owner,
        Health::new(100),
        type_ref,
        EntityCategory::Unit,
        0,
        5,
        true,
        37,
    );
    sim.substrate.entities.insert(entity);

    let bytes = GameSnapshot::save(&sim, 0, 0, "frame-37", 0);
    let loaded = GameSnapshot::load(&bytes).expect("load frame-37 entity");
    assert_eq!(
        loaded
            .sim
            .substrate
            .entities
            .get(1)
            .unwrap()
            .mission
            .dispatch_timer()
            .start_frame(),
        37
    );
}
