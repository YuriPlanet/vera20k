//! Display and layer tests: production air wrappers, Fly cross-level moves,
//! display lifecycle across save and restore, jumpjet layer queries and the
//! cell tracking of missiles and barrel elevation at unlimbo.

use super::*;

#[test]
fn production_air_wrapper_keeps_fly_exact_producer_and_reads_live_dummy() {
    let mut sim = Simulation::new();
    sim.resolved_terrain = Some(ResolvedTerrainGrid::from_cells(1, 1, Vec::new()));
    sim.resolved_terrain
        .as_ref()
        .unwrap()
        .test_set_dummy_cell_level_slope(3, 1);
    install_fly_aircraft(&mut sim, 1, SimFixed::from_num(125));
    let e = sim.substrate.entities.get_mut(1).unwrap();
    e.position.set_sub_x_for_test(SimFixed::from_num(64));
    e.position.set_sub_y_for_test(SimFixed::from_num(192));
    e.position.set_z_for_test(99);
    e.position.set_exact_z_leptons_for_test(Some(731));
    e.set_on_bridge(true);
    let xy = crate::sim::movement::ground_pose::position_world_xy(&e.position);
    let ground = crate::util::lepton::ground_height_leptons(3, 1, xy[0], xy[1]).unwrap();
    // Keep the physical height steady despite the stale controller cache.
    e.locomotor
        .as_mut()
        .unwrap()
        .set_fly_target_height(731 - ground - 416);
    sim.tick_air_movement_with_cell_lists_one(
        1,
        crate::rules::ruleset::RuleSet::empty_for_test(),
        None,
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    let e = sim.substrate.entities.get(1).unwrap();
    assert_eq!(e.position.exact_z_leptons(), Some(731));
    assert_eq!(sim.foot_navigation_coordinate(1).unwrap().z, 731);
    let dummy = sim
        .resolved_terrain
        .as_ref()
        .unwrap()
        .shared_cell_dummy()
        .snapshot();
    assert_eq!(dummy.level, 3);
    assert_eq!(dummy.slope_type, 1);
}

#[test]
fn production_air_wrapper_retains_native_jumpjet_result_below_the_ground() {
    fn fixture() -> Simulation {
        let mut sim = Simulation::new();
        install_common_raw_terrain(&mut sim, 8, 8, 2, None);
        insert_entity(&mut sim, 1, EntityCategory::Unit);
        let e = sim.substrate.entities.get_mut(1).unwrap();
        e.position.set_exact_z_leptons_for_test(Some(-37));
        let mut loco = LocomotorState::for_test_kind(LocomotorKind::Jumpjet);
        loco.set_balloon_hover_for_test(true);
        let runtime = loco.jumpjet_runtime_mut().unwrap();
        *runtime = runtime
            .clone()
            .with_phase_for_test(crate::sim::movement::jumpjet_movement::jumpjet_flight::STATE_HOLD)
            .with_moving_for_test(false);
        e.locomotor = Some(loco);
        sim
    }
    let mut direct = fixture();
    assert!(
        direct
            .tick_jumpjet_cruise_one(
                1,
                crate::rules::ruleset::RuleSet::empty_for_test(),
                &mut crate::sim::world::FrameEffects::default(),
            )
            .unwrap()
            .is_some()
    );
    let expected = direct
        .substrate
        .entities
        .get(1)
        .unwrap()
        .position
        .exact_z_leptons();
    assert_eq!(
        expected,
        Some(-37),
        "stationary native hold retains raw owner Z"
    );
    let mut wrapped = fixture();
    wrapped
        .tick_air_movement_with_cell_lists_one(
            1,
            crate::rules::ruleset::RuleSet::empty_for_test(),
            None,
            &mut crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
    assert_eq!(
        wrapped
            .substrate
            .entities
            .get(1)
            .unwrap()
            .position
            .exact_z_leptons(),
        expected
    );
    assert_eq!(wrapped.foot_navigation_coordinate(1).unwrap().z, -37);
}

#[test]
fn fly_cross_level_move_lands_on_destination_surface_after_restore() {
    use crate::util::fixed_math::{SIM_ONE, SIM_ZERO};

    // Fly4CDD07/4CDD1A: XY integration precedes physical-height feedback.
    // Speed 100 is the 255-lepton cap (`0x0071465F`), one cell in a frame.
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[AircraftTypes]\n0=TEST\n[TEST]\nSpeed=100\nLandable=yes\n\
             Locomotor={4A582746-9839-11D1-B709-00A024DDAFD1}\n",
        ))
        .unwrap();
    for (origin_level, destination_level) in [(0, 2), (2, 0)] {
        let mut sim = Simulation::with_seed(0);
        assert_eq!(sim.substrate.allocate_stable_id(), 1);
        install_common_raw_terrain(&mut sim, 8, 8, origin_level, None);
        sim.resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut_for_test(2, 2)
            .unwrap()
            .level = destination_level;
        install_fly_aircraft(&mut sim, 1, SimFixed::from_num(600));
        assert!(matches!(
            sim.try_reveal_entity(1, common_raw_request(2, 3, origin_level, 128, 128)),
            RevealOutcome::Revealed { .. }
        ));
        let origin_z = i32::from(origin_level) * 104 + 600;
        let destination_ground = i32::from(destination_level) * 104;
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        entity.position.set_exact_z_leptons_for_test(Some(origin_z));
        entity.body_facing.snap(0x0000, 0);
        let loco = entity.locomotor.as_mut().unwrap();
        loco.set_fly_target_height(600);

        let fly = loco.fly_runtime_mut().unwrap();
        fly.set_current_speed_for_test(SIM_ONE);
        fly.set_target_speed_for_test(SIM_ONE);
        let cells = sim
            .resolved_terrain
            .as_ref()
            .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
        let coordinate = crate::sim::movement::target_cell_coord(2, 2, cells.as_ref());
        assert!(Simulation::move_air_coordinate(
            &mut sim.interner,
            &sim.session,
            &mut sim.substrate,
            &mut sim.sound_events,
            &sim.resolved_terrain,
            1,
            coordinate,
            None,
            &rules
        ));
        sim.tick_air_movement_with_cell_lists_one(
            1,
            &rules,
            None,
            &mut crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        assert_eq!((entity.position.rx(), entity.position.ry()), (2, 2));
        let moved_z = entity.position.exact_z_leptons().unwrap();
        // Crossing itself does not add the ground delta. Only one bounded
        // vertical rate step adjusts the physical coordinate.
        assert!((moved_z - origin_z).abs() <= 21);
        assert_ne!(moved_z, origin_z);
        assert_eq!(
            crate::sim::movement::air_movement::current_fly_height(
                entity,
                sim.resolved_terrain.as_ref()
            ),
            moved_z - destination_ground,
        );
        // Stopped over its destination's cell, as the arrival leaves it, the
        // Fly lands there (`0x004CE3C0`'s Begin_Landing).
        let loco = entity.locomotor.as_mut().unwrap();
        let fly = loco.fly_runtime_mut().unwrap();
        fly.set_current_speed_for_test(SIM_ZERO);
        fly.set_target_speed_for_test(SIM_ZERO);
        loco.begin_fly_landing();
        loco.set_fly_target_height(0);

        let map_terrain = sim.resolved_terrain.as_ref().unwrap().clone();
        // In-scenario load reconstructs Scenario RNG from Seed0.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let bytes = GameSnapshot::save(&sim, 0, 0, "Fly terrain landing", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        restored.retain_process_state_for_load(crate::sim::world::SnapshotProcessState::Live(&sim));
        restored.resolved_terrain = Some(map_terrain);
        restored.restore_after_snapshot_load().unwrap();
        assert_eq!(restored.state_hash(), sim.state_hash());
        let mut landed = false;
        for frame in 1..80 {
            for instance in [&mut sim, &mut restored] {
                instance.session.set_tick_for_test(frame);
                instance.session.set_binary_frame_for_test(frame as u32);
                instance
                    .tick_air_movement_with_cell_lists_one(
                        1,
                        &rules,
                        None,
                        &mut crate::sim::world::FrameEffects::default(),
                    )
                    .unwrap();
            }
            assert_eq!(restored.state_hash(), sim.state_hash());
            let entity = sim.substrate.entities.get(1).unwrap();
            let height = crate::sim::movement::air_movement::current_fly_height(
                entity,
                sim.resolved_terrain.as_ref(),
            );
            if entity.locomotor.as_ref().unwrap().air_phase(height) == AirMovePhase::Landed {
                assert_eq!(entity.position.exact_z_leptons(), Some(destination_ground));
                assert_eq!(
                    sim.foot_navigation_coordinate(1).unwrap().z,
                    destination_ground
                );
                landed = true;
                break;
            }
        }
        assert!(landed, "Fly must finish its actual descent");
    }
}

#[test]
fn pre_logic_save_observes_input_and_unsorted_ground_before_first_tick() {
    use super::{FrameEffects, TickLane, display_layers::DisplayLayer};
    use crate::sim::command::{Command, CommandEnvelope};

    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Player");
    sim.houses
        .register(HouseState::new(owner, 0, None, true, 0, 10));
    for id in 1..=4 {
        insert_entity(&mut sim, id, EntityCategory::Unit);
        assert!(matches!(
            sim.try_reveal_entity(id, common_raw_request(2 + id as u16, 3, 0, 128, 128)),
            RevealOutcome::Revealed { .. }
        ));
    }
    assert_eq!(
        sim.substrate.display.members(DisplayLayer::GROUND),
        [1, 2, 3, 4]
    );
    for id in 1..=4 {
        sim.remove_entity_occupancy(id);
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .position
            .set_rx_for_test(7 - id as u16);
        sim.add_entity_occupancy(id);
    }
    let mut runtime = crate::sim::runtime::SimRuntime::from_simulation(sim);
    let saved = std::cell::RefCell::new(None);
    let mut save = |sim: &Simulation| {
        *saved.borrow_mut() = Some(GameSnapshot::save_validated(sim, 0, 0, "initial", 0));
    };
    runtime
        .advance_frame(
            &[CommandEnvelope::new(
                owner,
                1,
                Command::SetGameSpeed { speed: 2 },
            )],
            67,
            TickLane::Ordinary,
            &mut FrameEffects::default().with_before_logic(&mut save),
        )
        .unwrap();
    let loaded = GameSnapshot::load(&saved.into_inner().expect("reached pre-Logic save")).unwrap();
    assert_eq!(loaded.sim.session().tick(), 0);
    assert_eq!(loaded.sim.session().game_options().game_speed, 2);
    // The simulation no longer sorts Ground before Logic
    // (`DisplayLayers::submit`): the moved members keep their places.
    assert_eq!(
        loaded.sim.substrate.display.members(DisplayLayer::GROUND),
        [1, 2, 3, 4]
    );
    assert_eq!(runtime.simulation.session().tick(), 1);
}

#[test]
fn display_lifecycle_is_independent_of_logic_and_survives_production_save() {
    use super::display_layers::DisplayLayer;
    let mut sim = Simulation::new();
    for _ in 0..4 {
        let id = sim.substrate.allocate_stable_id();
        insert_entity(&mut sim, id, EntityCategory::Unit);
        let mut req = common_raw_request(2 + id as u16, 3, 0, 128, 128);
        req.logic_eligible = id != 2;
        assert!(matches!(
            sim.try_reveal_entity(id, req),
            RevealOutcome::Revealed { .. }
        ));
    }
    assert_eq!(sim.substrate.logic.as_slice(), [1, 3, 4]);
    assert_eq!(
        sim.substrate.display.members(DisplayLayer::GROUND),
        [1, 2, 3, 4]
    );
    // Coordinates change without resubmitting: the simulation's Ground
    // keeps its members where they are, and only the draw order follows
    // the keys (`Simulation::ground_draw_order`).
    for id in 1..=4 {
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .position
            .set_rx_for_test(7 - id as u16);
    }
    assert_eq!(
        sim.substrate.display.members(DisplayLayer::GROUND),
        [1, 2, 3, 4]
    );
    assert_eq!(sim.ground_draw_order(None), [4, 3, 2, 1]);
    let bytes = GameSnapshot::save(&sim, 0, 0, "display-order", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(
        restored.substrate.display.members(DisplayLayer::GROUND),
        [1, 2, 3, 4]
    );
    let hash = restored.state_hash();
    // A removal swaps the last member into the hole.
    restored.object_conceal(2);
    assert_eq!(
        restored.substrate.display.members(DisplayLayer::GROUND),
        [1, 4, 3]
    );
    assert_eq!(restored.substrate.logic.as_slice(), [1, 3, 4]);
    assert_ne!(hash, restored.state_hash());
    restored
        .substrate
        .display
        .submit(999, Some(DisplayLayer::TOP));
    assert!(matches!(
        restored.restore_after_snapshot_load(),
        Err(SnapshotRestoreError::MissingDisplayIdentity { object_id: 999 })
    ));
}

#[test]
fn mixed_display_lifecycle_matches_original_sequences_and_save_restore() {
    use super::display_layers::DisplayLayer;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::rules::voxel_anim_type::{VoxelAnimType, VoxelAnimTypeId};
    let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/display_non_entity.json",
    ))
    .unwrap();
    assert_eq!(rows.len(), 9);
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[BuildingTypes]\n0=TEST\n[TEST]\nStrength=100\nTurretAnimIsVoxel=yes\n",
    ))
    .unwrap();
    let voxel_type = VoxelAnimType::from_ini_section(
        "TIRE",
        IniFile::from_str("[TIRE]\nDuration=150\n")
            .section("TIRE")
            .unwrap(),
    );
    for row in rows {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let actors = input["actors"].as_array().unwrap();
        let mut sim = Simulation::new();
        // Native Scenario Load reseeds Random(0). These display operations
        // consume no Scenario draws, so the complete saved hash can match.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        for (index, actor) in actors.iter().enumerate() {
            let id = sim.substrate.allocate_stable_id();
            assert_eq!(id, index as u64 + 1);
            let xyz: [i32; 3] = std::array::from_fn(|i| actor["xyz"][i].as_i64().unwrap() as i32);
            match actor["kind"].as_str().unwrap() {
                "unit" | "building" => {
                    let category = if actor["kind"] == "unit" {
                        EntityCategory::Unit
                    } else {
                        EntityCategory::Structure
                    };
                    insert_entity(&mut sim, id, category);
                    let entity = sim.substrate.entities.get_mut(id).unwrap();
                    entity.position.set_rx_for_test((xyz[0] / 256) as u16);
                    entity.position.set_ry_for_test((xyz[1] / 256) as u16);
                    entity
                        .position
                        .set_sub_x_for_test(SimFixed::from_num(xyz[0] % 256));
                    entity
                        .position
                        .set_sub_y_for_test(SimFixed::from_num(xyz[1] % 256));
                    entity.position.set_exact_z_leptons_for_test(Some(xyz[2]));
                }
                "particle" => {
                    insert_particle_system(&mut sim, id);
                    sim.substrate
                        .particle_systems
                        .get_mut(id)
                        .unwrap()
                        .set_coords_for_test(IVec3::from_array(xyz));
                }
                "terrain" => {
                    let terrain = {
                        let mut terrain = TerrainObjectState::for_test(
                            id,
                            sim.interner.intern("TREE"),
                            (xyz[0] / 256) as u16,
                            (xyz[1] / 256) as u16,
                        );
                        terrain.set_health_for_test(10);
                        terrain.set_max_health_for_test(10);
                        terrain
                    };
                    sim.terrain_objects
                        .cell_index_mut_for_test()
                        .insert(terrain.cell(), id);
                    sim.terrain_objects
                        .objects_mut_for_test()
                        .insert(id, terrain);
                }
                "bullet" => {
                    let mut shot = gsi_05_02_projectile(0, None);
                    shot.flat = actor["flat"].as_bool().unwrap();
                    shot.origin = ProjectileCoord::new(xyz[0], xyz[1], xyz[2]);
                    sim.admit_projectile(id, shot);
                }
                "wave" => {
                    sim.admit_wave(
                        id,
                        Wave::new(
                            3,
                            ProjectileCoord::new(xyz[0], xyz[1], xyz[2]),
                            ProjectileCoord::new(xyz[0] + 256, xyz[1], xyz[2]),
                        ),
                    );
                }
                "voxel" => {
                    let debris = crate::sim::voxel_anim::spawn_debris_piece(
                        id,
                        0,
                        VoxelAnimTypeId(0),
                        &voxel_type,
                        None,
                        IVec3::from_array(xyz),
                        &mut crate::sim::rng::SimRng::new(1),
                    )
                    .unwrap();
                    sim.substrate.voxel_anims.insert(debris);
                }
                kind => panic!("unexpected {kind}"),
            }
        }
        for (step, expected) in input["steps"]
            .as_array()
            .unwrap()
            .iter()
            .zip(row["after_steps"].as_array().unwrap())
        {
            let id = step["actor"].as_u64().map_or(0, |i| i + 1);
            match step["op"].as_str().unwrap() {
                "submit" => match actors[id as usize - 1]["kind"].as_str().unwrap() {
                    "unit" | "building" => Simulation::submit_entity_display(
                        &sim.interner,
                        &mut sim.substrate,
                        &sim.resolved_terrain,
                        id,
                        &rules,
                        None,
                    ),
                    "particle" => {
                        sim.reveal_particle_system(id);
                    }
                    "terrain" => {
                        sim.register_terrain_object(id, &rules);
                    }
                    "bullet" => {
                        sim.register_projectile(
                            id,
                            actors[id as usize - 1]["flat"].as_bool().unwrap(),
                        );
                    }
                    "wave" => {
                        sim.register_wave(id);
                    }
                    "voxel" => {
                        sim.reveal_voxel_anim(id);
                    }
                    _ => unreachable!(),
                },
                "remove" => {
                    sim.substrate.display.remove(id);
                }
                // The simulation keeps Ground unordered (`DisplayLayers::submit`).
                "sort" => {}
                "coordinates" => {
                    let xyz: [i32; 3] =
                        std::array::from_fn(|i| step["xyz"][i].as_i64().unwrap() as i32);
                    if let Some(entity) = sim.substrate.entities.get_mut(id) {
                        entity.position.set_rx_for_test((xyz[0] / 256) as u16);
                        entity.position.set_ry_for_test((xyz[1] / 256) as u16);
                        entity
                            .position
                            .set_sub_x_for_test(SimFixed::from_num(xyz[0] % 256));
                        entity
                            .position
                            .set_sub_y_for_test(SimFixed::from_num(xyz[1] % 256));
                    } else {
                        sim.substrate
                            .particle_systems
                            .get_mut(id)
                            .unwrap()
                            .set_coords_for_test(IVec3::from_array(xyz));
                    }
                }
                _ => unreachable!(),
            }
            for layer in 0..5 {
                let mut expected: Vec<u64> = expected[layer]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|i| i.as_u64().unwrap() + 1)
                    .collect();
                let layer = DisplayLayer::from_index(layer as u8).unwrap();
                let mut members = sim.substrate.display.members(layer).to_vec();
                // Ground is a set: compare its membership.
                if layer == DisplayLayer::GROUND {
                    expected.sort_unstable();
                    members.sort_unstable();
                }
                assert_eq!(members, expected, "{name}: {step}");
            }
        }
        let bytes = GameSnapshot::save(&sim, 0, 0, "mixed-display", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        assert_eq!(restored.state_hash(), sim.state_hash(), "{name}");
        for id in 1..=actors.len() as u64 {
            if restored.substrate.entities.contains(id) {
                // Corpus entities were submitted directly, independently of
                // Reveal/Logic; their production Conceal is tested separately.
                restored.substrate.display.remove(id);
            } else {
                restored.unregister_non_entity_object(id);
            }
            assert_eq!(restored.substrate.display.layer_of(id), None);
        }
    }
}

#[test]
fn jumpjet_process_compares_live_layer_queries_not_cached_registration() {
    use super::display_layers::DisplayLayer;
    let mut sim = Simulation::new();
    let id = sim.substrate.allocate_stable_id();
    insert_entity(&mut sim, id, EntityCategory::Unit);
    sim.substrate.entities.get_mut(id).unwrap().locomotor =
        Some(LocomotorState::for_test_kind(LocomotorKind::Jumpjet));
    sim.try_reveal_entity(id, common_raw_request(3, 3, 0, 128, 128));
    // Explicit stale display cache witness. Native compares its two +74
    // queries and leaves this history alone while the Process answer is stable.
    sim.substrate.display.submit(id, Some(DisplayLayer::TOP));
    sim.tick_air_movement_with_cell_lists_one(
        id,
        crate::rules::ruleset::RuleSet::empty_for_test(),
        None,
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    assert_eq!(sim.substrate.display.layer_of(id), Some(DisplayLayer::TOP));

    // A real changed query re-submits even if cached membership is absent.
    sim.substrate.display.remove(id);
    let before = sim
        .entity_display_layer(id, crate::rules::ruleset::RuleSet::empty_for_test())
        .unwrap();
    // GetHeight reads the coordinate Z Jumpjet flight writes.
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .position
        .set_exact_z_leptons_for_test(Some(600));
    Simulation::complete_jumpjet_display_process(
        &sim.interner,
        &mut sim.substrate,
        &sim.resolved_terrain,
        id,
        before,
        crate::rules::ruleset::RuleSet::empty_for_test(),
    );
    assert_eq!(sim.substrate.display.layer_of(id), Some(DisplayLayer::TOP));
    // The native tail's alive gate prevents another submission after death.
    let before = sim
        .entity_display_layer(id, crate::rules::ruleset::RuleSet::empty_for_test())
        .unwrap();
    let entity = sim.substrate.entities.get_mut(id).unwrap();
    entity.lifecycle.set_object_alive_for_test(false);
    entity.position.set_exact_z_leptons_for_test(Some(0));
    Simulation::complete_jumpjet_display_process(
        &sim.interner,
        &mut sim.substrate,
        &sim.resolved_terrain,
        id,
        before,
        crate::rules::ruleset::RuleSet::empty_for_test(),
    );
    assert_eq!(sim.substrate.display.layer_of(id), Some(DisplayLayer::TOP));
}

/// Fly's phase tail (4CD4DE) resubmits Display after callbacks that may have
/// UnInit an owner (landing retry's C4 receiver). Mark5F5850 refuses the
/// Limbo owner; store removal expires the Display registration so no later
/// Ground walk meets a removed identity.
#[test]
fn display_registration_expires_with_a_resubmitted_uninit_owner() {
    use super::display_layers::DisplayLayer;
    let mut sim = Simulation::new();
    let id = sim.substrate.allocate_stable_id();
    insert_entity(&mut sim, id, EntityCategory::Unit);
    sim.try_reveal_entity(id, common_raw_request(3, 3, 0, 128, 128));
    sim.uninit(id);
    assert!(sim.substrate.entities.get(id).unwrap().lifecycle.in_limbo());
    Simulation::submit_entity_display(
        &sim.interner,
        &mut sim.substrate,
        &sim.resolved_terrain,
        id,
        crate::rules::ruleset::RuleSet::empty_for_test(),
        None,
    );
    assert_eq!(
        sim.substrate.display.layer_of(id),
        Some(DisplayLayer::GROUND)
    );
    sim.process_pending_delete();
    assert!(!sim.substrate.entities.contains(id));
    assert_eq!(sim.substrate.display.layer_of(id), None);
    assert_eq!(sim.ground_draw_order(None), Vec::<u64>::new());
}

/// `TechnoClass::Unlimbo` snaps the barrel elevation level (`0x006F6DC3`) and
/// turns it toward `0x4000 - (FireAngle= << 8)` (`0x006F6DF5`) at the
/// constructor's rate 3. A Techno whose elevation never moved hashes as
/// schemas before the fold.
#[test]
fn unlimbo_levels_then_aims_the_barrel_elevation() {
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[VehicleTypes]\n0=TANK\n1=RAISED\n\
         [TANK]\nStrength=100\nSpeed=6\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
         [RAISED]\nStrength=100\nSpeed=6\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
         FireAngle=32\n",
        ))
        .expect("barrel elevation rules");
    // FireAngle 8 turns 0x800 in two frames; 32 turns 0x2000 in ten, at
    // 0x2000 / 10 a frame, so it starts two short of level.
    for (type_id, samples) in [
        ("TANK", [(40, 0x4000), (41, 0x3C00), (42, 0x3800)]),
        ("RAISED", [(40, 0x3FFE), (45, 0x2FFF), (50, 0x2000)]),
    ] {
        let mut sim = Simulation::with_seed(7);
        sim.session.set_binary_frame_for_test(40);
        sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
            base: 0,
            off_fc: -100,
            off_100: -100,
            off_104: 200,
            off_108: 200,
        });
        install_common_raw_terrain(&mut sim, 10, 10, 0, None);
        let cell = sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut_for_test(4, 4)
            .unwrap();
        cell.speed_costs.track = Some(crate::util::native_x87::NativeF32Bits::ONE);
        cell.speed_costs.wheel = Some(crate::util::native_x87::NativeF32Bits::ONE);
        cell.base_speed_costs = cell.speed_costs;
        let id = sim
            .spawn_object(type_id, "Americans", 4, 4, 0, &rules)
            .expect("production spawn/unlimbo");
        let elevation = *sim.substrate.entities.get(id).unwrap().barrel_elevation();
        for (frame, heading) in samples {
            assert_eq!(elevation.current(frame), heading, "{type_id} frame {frame}");
        }
        assert!(
            !sim.substrate
                .entities
                .get(id)
                .unwrap()
                .barrel_elevation_is_constructed(),
            "{type_id}"
        );
    }

    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    assert!(matches!(
        sim.reveal(1),
        crate::sim::world::RevealOutcome::Revealed { .. }
    ));
    assert!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .barrel_elevation_is_constructed()
    );
}

/// Rocket Process moves its owner's AircraftTracker entry after the flight
/// step (`0x00662FA1..0x00662FD0`, the update at `0x004138C0`): a missile
/// whose step reaches a new cell is tracked there in the same turn.
#[test]
fn a_missile_is_tracked_in_the_cell_its_flight_step_reached() {
    // One frame of cruise accelerates to the clamped Speed=100 (255 leptons).
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n0=TEST\n[BuildingTypes]\n\
         [General]\nDMislAcceleration=255\n[TEST]\nSpeed=100\n",
        ))
        .expect("missile rules");
    let mut sim = Simulation::new();
    install_common_raw_terrain(&mut sim, 32, 32, 0, None);
    // A map Size whose In_Bounds diamond (`0x00568300`) holds the flight.
    sim.playfield_bounds = Some(crate::sim::arena_fixture::OPEN_PLAYFIELD);
    sim.playfield_size_height = Some(20);
    insert_entity(&mut sim, 1, EntityCategory::Aircraft);
    sim.substrate.entities.get_mut(1).unwrap().locomotor =
        Some(LocomotorState::for_test_kind(LocomotorKind::Rocket));
    let _ = sim.try_reveal_entity(1, common_raw_request(12, 12, 0, 128, 128));
    // It cruises 512 leptons over the ground.
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .position
        .set_exact_z_leptons_for_test(Some(512));
    Simulation::rocket_move_to(
        &sim.interner,
        &sim.session,
        &mut sim.substrate,
        1,
        crate::sim::components::DriveCoord::cell(25, 12, 0),
        &rules,
    );
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .locomotor
        .as_mut()
        .unwrap()
        .rocket_runtime_mut()
        .unwrap()
        .set_mission_state_for_test(4);
    // The tilt's end added the climbing missile to the AircraftTracker.
    Simulation::aircraft_tracker_add(&sim.session, &mut sim.substrate, 1);
    let bucket = |sim: &Simulation, cell: (u16, u16)| {
        crate::sim::occupancy::air_spatial_bucket_index(
            cell.0,
            cell.1,
            sim.session.map_width(),
            sim.session.map_height(),
        )
    };
    assert_eq!(
        sim.substrate.entities.get(1).unwrap().air_spatial_bucket(),
        Some(bucket(&sim, (12, 12)))
    );

    sim.advance_live_object_turn(1, &rules, &mut super::techno_ai::ObjectAiCtx::default())
        .unwrap();

    let missile = sim.substrate.entities.get(1).unwrap();
    let cell = (missile.position.rx(), missile.position.ry());
    assert_eq!(cell, (13, 12), "the flight step left the launch cell");
    assert_eq!(missile.air_spatial_bucket(), Some(bucket(&sim, cell)));
}
