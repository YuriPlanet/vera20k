//! Tests of the state hash: which state each fold reaches and which owner
//! digest it lands in.

use super::*;

#[cfg(all(test, tests_sim_world_t_z))]
mod retained_cell_hash_tests {
    use super::*;
    use crate::map::resolved_terrain::ResolvedTerrainGrid;
    use crate::sim::combat::TargetKind;
    use crate::sim::components::NavTargetRef;
    use crate::sim::game_entity::GameEntity;

    #[test]
    fn retained_cell_targets_hash_live_dummy_coordinate_without_restamping_it() {
        let mut sim = Simulation::new();
        let mut terrain = ResolvedTerrainGrid::from_cells(0, 0, Vec::new());
        terrain.bind_shared_cell_dummy(sim.shared_cell_dummy.clone());
        sim.resolved_terrain = Some(terrain);
        let dummy = sim.effective_shared_cell_dummy();
        let mut entity = GameEntity::test_default(1, "FV", "Test", 10, 10);
        // Every cell is deliberately unallocated. These different requested
        // coordinates retain the same process object; hashing may not stamp it.
        for retained_field in 0..8 {
            entity.attack_target.set_for_test(None);
            entity.seldom.suspended_attack_target.set_for_test(None);
            entity.set_archive_target(None);
            entity.navigation = Default::default();
            match retained_field {
                0 => entity
                    .attack_target
                    .set_for_test(Some(TargetKind::Cell(1, 2))),
                1 => entity
                    .seldom
                    .suspended_attack_target
                    .set_for_test(Some(TargetKind::Cell(3, 4))),
                2 => entity.set_archive_target(Some(TargetKind::Cell(5, 6))),
                3 => entity
                    .navigation
                    .set_nav_com_for_test(Some(NavTargetRef::cell(7, 8))),
                4 => entity
                    .navigation
                    .set_nav_com_aux_for_test(Some(NavTargetRef::cell(9, 10))),
                5 => entity
                    .navigation
                    .set_suspended_nav_com_for_test(Some(NavTargetRef::cell(11, 12))),
                6 => entity
                    .navigation
                    .push_nav_queue_for_test(NavTargetRef::cell(13, 14)),
                7 => entity
                    .navigation
                    .set_nav_list_for_test(vec![NavTargetRef::cell(15, 16)], false),
                _ => unreachable!(),
            }
            sim.substrate.entities.insert(entity.clone());
            dummy.stamp_coord(99, 98);
            let before = sim.state_hash();
            assert_eq!(dummy.snapshot().coord, (99, 98));
            dummy.stamp_coord(97, 96);
            assert_ne!(before, sim.state_hash(), "retained field {retained_field}");
            assert_eq!(dummy.snapshot().coord, (97, 96));
        }
    }

    #[test]
    fn an_allocated_cell_bridge_anchor_on_the_dummy_hashes_its_live_coordinate() {
        let mut sim = Simulation::new();
        let mut anchored = crate::map::resolved_terrain::test_flat_cell(1, 0);
        anchored.bridge_facts.native_anchor =
            Some(crate::map::cell_index::NativeCellIdentity::Dummy);
        let cells = vec![crate::map::resolved_terrain::test_flat_cell(0, 0), anchored];
        sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(2, 1, cells));
        let dummy = sim.effective_shared_cell_dummy();
        let coordinate_hashes = |sim: &Simulation| {
            dummy.stamp_coord(5, 6);
            let before = sim.state_hash();
            dummy.stamp_coord(7, 8);
            before != sim.state_hash()
        };
        assert!(coordinate_hashes(&sim));
        let terrain = sim.resolved_terrain.as_mut().expect("installed map");
        terrain.test_set_native_allocated_cells(&[(0, 0)]);
        assert!(
            !coordinate_hashes(&sim),
            "an unallocated cell retains nothing"
        );
        let terrain = sim.resolved_terrain.as_mut().expect("installed map");
        terrain.test_set_native_allocated_cells(&[(0, 0), (1, 0)]);
        let anchor = &mut terrain
            .cell_mut_for_test(1, 0)
            .expect("the cell")
            .bridge_facts;
        anchor.native_anchor = None;
        assert!(
            !coordinate_hashes(&sim),
            "a released anchor retains nothing"
        );
    }
}

#[cfg(all(test, tests_sim_world_t_z))]
mod drive_ship_slope_hash_tests {
    use super::Simulation;
    use crate::map::entities::EntityCategory;
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::locomotor::LocomotorState;
    use crate::sim::movement::slope_transition::SlopeTransitionState;

    fn hash_with_state(kind: LocomotorKind, stashed: bool, state: SlopeTransitionState) -> u64 {
        let mut sim = Simulation::new();
        let mut entity = GameEntity::test_default(1, "SLOPE", "Americans", 2, 2);
        entity.set_category_for_test(EntityCategory::Unit);
        let mut locomotor = LocomotorState::for_test_kind(kind);
        *locomotor.active_slope_transition_mut().unwrap() = state;
        if stashed {
            assert!(locomotor.begin_piggyback(LocomotorKind::Teleport, 90,));
        }
        entity.locomotor = Some(locomotor);
        sim.substrate.entities.insert(entity);
        sim.state_hash()
    }

    #[test]
    fn every_active_and_stashed_drive_ship_slope_field_changes_current_hash() {
        let base = SlopeTransitionState::from_fields_for_test(1, 2, 30, 3);
        let variants = [
            SlopeTransitionState::from_fields_for_test(9, 2, 30, 3),
            SlopeTransitionState::from_fields_for_test(1, 9, 30, 3),
            SlopeTransitionState::from_fields_for_test(1, 2, -1, 3),
            SlopeTransitionState::from_fields_for_test(1, 2, 30, 0),
        ];
        for kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
            for stashed in [false, true] {
                let baseline = hash_with_state(kind, stashed, base);
                for variant in variants {
                    assert_ne!(
                        baseline,
                        hash_with_state(kind, stashed, variant),
                        "kind={kind:?} stashed={stashed} must hash every slope field"
                    );
                }
            }
        }
    }
}

#[cfg(all(test, tests_sim_world_t_z))]
mod locomotor_field_hash_tests {
    use super::Simulation;
    use crate::map::entities::EntityCategory;
    use crate::rules::locomotor_type::{LocomotorKind, MovementZone, SpeedType};
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};

    /// A Teleport unit, optionally on a Drive leg, with `mutate` applied to the
    /// active object or, on the leg, to the suspended Teleport.
    fn hash_with(stashed: bool, mutate: Option<fn(&mut LocomotorState)>) -> u64 {
        let mut sim = Simulation::new();
        let mut entity = GameEntity::test_default(1, "LOCO", "Americans", 2, 2);
        entity.set_category_for_test(EntityCategory::Unit);
        let mut locomotor = LocomotorState::for_test_kind(LocomotorKind::Teleport);
        if stashed {
            assert!(locomotor.begin_piggyback(LocomotorKind::Drive, 0));
        }
        if let Some(mutate) = mutate {
            match locomotor.piggyback_mut() {
                Some(stash) => mutate(stash.suspended_mut_for_test()),
                None => mutate(&mut locomotor),
            }
        }
        entity.locomotor = Some(locomotor);
        sim.substrate.entities.insert(entity);
        sim.state_hash()
    }

    /// Every field outside the class payload (`drive_ship_slope_hash_tests`
    /// and the payload tests cover it) changes the hash, on the active object
    /// and on a suspended one. The six fields #680 found missing from the
    /// active fold are among them.
    #[test]
    fn every_active_and_stashed_locomotor_field_changes_current_hash() {
        type Mutation = (&'static str, fn(&mut LocomotorState));
        let mutations: [Mutation; 6] = [
            ("kind", |l| l.set_kind_for_test(LocomotorKind::Walk)),
            ("powered", |l| l.set_powered_for_test(false)),
            ("layer", |l| l.set_layer_for_test(MovementLayer::Bridge)),
            ("balloon_hover", |l| l.set_balloon_hover_for_test(true)),
            ("speed_type", |l| {
                l.set_speed_type_for_test(SpeedType::Wheel)
            }),
            ("movement_zone", |l| {
                l.set_movement_zone_for_test(MovementZone::Amphibious)
            }),
        ];
        assert_ne!(hash_with(false, None), hash_with(true, None));
        for stashed in [false, true] {
            let baseline = hash_with(stashed, None);
            for (field, mutate) in mutations {
                assert_ne!(
                    baseline,
                    hash_with(stashed, Some(mutate)),
                    "stashed={stashed} must hash {field}"
                );
            }
        }
    }
}

#[cfg(all(test, tests_sim_world_t_z))]
mod playfield_authority_hash_tests {
    use super::Simulation;
    use crate::map::playfield::PlayfieldBounds;

    fn playfield() -> PlayfieldBounds {
        PlayfieldBounds {
            base: 80,
            off_fc: 2,
            off_100: 4,
            off_104: 76,
            off_108: 48,
        }
    }

    #[test]
    fn state_hash_includes_mutable_playfield_authority() {
        let mut baseline = Simulation::new();
        baseline.playfield_bounds = Some(playfield());
        baseline.playfield_size_height = Some(58);

        let mut changed_bounds = Simulation::new();
        changed_bounds.playfield_bounds = Some(PlayfieldBounds {
            off_100: 5,
            ..playfield()
        });
        changed_bounds.playfield_size_height = Some(58);

        let mut changed_size_height = Simulation::new();
        changed_size_height.playfield_bounds = Some(playfield());
        changed_size_height.playfield_size_height = Some(59);

        let mut changed_revision = Simulation::new();
        changed_revision.playfield_bounds = Some(playfield());
        changed_revision.playfield_size_height = Some(58);
        changed_revision.playfield_revision = 1;

        assert_ne!(baseline.state_hash(), changed_bounds.state_hash());
        assert_ne!(baseline.state_hash(), changed_size_height.state_hash());
        assert_ne!(baseline.state_hash(), changed_revision.state_hash());
    }
}

#[cfg(all(test, tests_sim_world_t_z))]
mod hash_owner_attribution_tests {
    use super::Simulation;

    /// A world that differs in one part names that part, so a match report
    /// can say what two clients disagreed on.
    #[test]
    fn a_scenario_rng_difference_shows_under_its_own_name() {
        let first = Simulation::new();
        let mut second = Simulation::new();
        second.scenario_rng.next_u32();
        let differing: Vec<_> = first
            .state_hash_owners()
            .into_iter()
            .zip(second.state_hash_owners())
            .filter(|(left, right)| left != right)
            .map(|(left, _)| left.0)
            .collect();
        assert_eq!(differing, ["fold_before_fog", "scenario_rng"]);
    }

    /// The causes of the failed 0.3.5 starts: carried trigger locals and
    /// the network MapGen cursor each show under their own name.
    #[test]
    fn network_trigger_locals_and_mapgen_show_under_their_own_names() {
        use crate::rules::ini_parser::IniFile;
        use crate::sim::scenario_session::ScenarioGameMode;
        let map = |variables: &str| {
            let mut map = crate::map::rmg::emit::empty_map_file(
                &crate::map::rmg::RmgOptions::default(),
                16,
                16,
            );
            map.local_variables = crate::map::variable_names::parse_local_variables(
                &IniFile::from_str(&format!("[VariableNames]\n{variables}")),
            );
            map
        };
        let mut previous = Simulation::new();
        previous.initialize_trigger_definitions(
            &map("7=Previous,1\n"),
            &Default::default(),
            None,
            None,
            None,
        );
        let start = |carried| {
            let mut sim = Simulation::new();
            sim.install_session_mode(ScenarioGameMode::Lan);
            if let Some(carried) = carried {
                sim.inherit_local_variables_for_fresh_load(carried);
            }
            sim.initialize_trigger_definitions(&map(""), &Default::default(), None, None, None);
            sim
        };
        let first = start(None);
        let mut second = start(Some(previous.retain_local_variables_for_fresh_load()));
        second.mapgen_rng.next_u32();
        let differing: Vec<_> = first
            .state_hash_owners()
            .into_iter()
            .zip(second.state_hash_owners())
            .filter(|(left, right)| left != right)
            .map(|(left, _)| left.0)
            .collect();
        assert_eq!(
            differing,
            ["fold_before_fog", "trigger_locals", "mapgen_rng"]
        );
    }
}

#[cfg(all(test, tests_sim_world_t_z))]
mod shared_dummy_bridge_hash_tests {
    use super::Simulation;
    use glam::IVec3;

    use crate::map::bridge_facts::BRIDGE_FLAG_ANCHOR_SELF;
    use crate::map::resolved_terrain::ResolvedTerrainGrid;
    use crate::map::voxel_slope::slope_matrix;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::particles::spark::SparkMotionStep;
    use crate::sim::particles::spark_world::SparkCollisionWorld;
    use crate::util::native_x87::NativeF32Bits;

    fn spark_motion_at(x: i32, y: i32, z: i32) -> SparkMotionStep {
        SparkMotionStep {
            old_coords: IVec3::new(x, y, z),
            candidate_coords: IVec3::new(x, y, z),
            candidate_f32: [
                NativeF32Bits::from_bits((x as f32).to_bits()),
                NativeF32Bits::from_bits((y as f32).to_bits()),
                NativeF32Bits::from_bits((z as f32).to_bits()),
            ],
            persistent_velocity: [NativeF32Bits::POSITIVE_ZERO; 3],
            probe_velocity: [NativeF32Bits::POSITIVE_ZERO; 3],
        }
    }

    #[test]
    fn gsi_04_01_hashes_dummy_bridge_bits_without_retained_projectile() {
        let sim = Simulation::new();
        let dummy = sim.effective_shared_cell_dummy();
        let clear_hash = sim.state_hash();

        dummy.set_bridge_flags_0x1180(BRIDGE_FLAG_ANCHOR_SELF);
        let bridge_hash = sim.state_hash();
        assert_ne!(
            clear_hash, bridge_hash,
            "live anchor bit 0x80 alone is future-affecting hash authority"
        );

        dummy.stamp_coord(17, -3);
        assert_eq!(
            bridge_hash,
            sim.state_hash(),
            "without a retained Bullet the requested coordinate stays excluded; 0x1180, level, and slope are unconditional"
        );
    }

    #[test]
    fn bridge_publication_hashes_full_dummy_flags_and_retained_anchor_coordinate() {
        use crate::map::cell_index::NativeCellIdentity;
        let sim = Simulation::new();
        let dummy = sim.effective_shared_cell_dummy();
        let baseline = sim.state_hash();
        dummy.write_raw_flags(0x10000);
        let side_flag = sim.state_hash();
        assert_ne!(baseline, side_flag);
        dummy.write_native_anchor(Some(NativeCellIdentity::Dummy));
        let retained = sim.state_hash();
        assert_ne!(side_flag, retained);
        dummy.stamp_coord(7, -3);
        assert_ne!(
            retained,
            sim.state_hash(),
            "retained bridge pointer makes dummy coordinate future-affecting"
        );
        dummy.write_native_anchor(None);
        assert_eq!(side_flag, sim.state_hash());
    }

    #[test]
    fn gsi_04_03_hashes_dummy_level_slope_without_retained_projectile() {
        let mut sim = Simulation::new();
        sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(0, 0, Vec::new()));
        let dummy = sim.effective_shared_cell_dummy();
        let terrain_dummy = sim.resolved_terrain.as_ref().unwrap().shared_cell_dummy();
        assert!(
            dummy.same_identity(&terrain_dummy),
            "the hash authority must be the dummy bound into production terrain"
        );
        let rules = RuleSet::from_ini(&IniFile::from_str("")).unwrap();
        let clear_hash = sim.state_hash();

        dummy.set_level_slope(-3, 0);
        let level_facts = SparkCollisionWorld::new(&sim, &rules)
            .unwrap()
            .query(spark_motion_at(320, 192, 300))
            .unwrap();
        assert_eq!(level_facts.ground_z, -311);
        assert!(level_facts.slope_matrix.is_none());
        let level_only_hash = sim.state_hash();
        assert_ne!(
            clear_hash, level_only_hash,
            "dummy level alone is unconditional hash authority"
        );

        dummy.set_level_slope(0, 0);
        assert_eq!(clear_hash, sim.state_hash());

        dummy.set_level_slope(0, 9);
        let slope_facts = SparkCollisionWorld::new(&sim, &rules)
            .unwrap()
            .query(spark_motion_at(320, 192, -100))
            .unwrap();
        assert_eq!(slope_facts.ground_z, 104);
        assert_eq!(slope_facts.slope_matrix, Some(slope_matrix(9).unwrap()));
        let slope_only_hash = sim.state_hash();
        assert_ne!(
            clear_hash, slope_only_hash,
            "dummy slope alone is unconditional hash authority"
        );
    }
}

#[cfg(all(test, tests_sim_world_t_z))]
mod raw_cell_occupation_hash_tests {
    use super::Simulation;

    #[test]
    fn gsi_04_12_raw_occupation_hash_distinguishes_byte_plane_and_coordinate() {
        let empty = Simulation::new();

        let mut ground = Simulation::new();
        ground.substrate.raw_cell_occupation.mark_ground(4, 9, 0x20);

        let mut different_byte = Simulation::new();
        different_byte
            .substrate
            .raw_cell_occupation
            .mark_ground(4, 9, 0x40);

        let mut deck = Simulation::new();
        deck.substrate.raw_cell_occupation.mark_deck(4, 9, 0x20);

        let mut different_coordinate = Simulation::new();
        different_coordinate
            .substrate
            .raw_cell_occupation
            .mark_ground(9, 4, 0x20);

        assert_ne!(empty.state_hash(), ground.state_hash());
        assert_ne!(ground.state_hash(), different_byte.state_hash());
        assert_ne!(ground.state_hash(), deck.state_hash());
        assert_ne!(ground.state_hash(), different_coordinate.state_hash());
    }

    #[test]
    fn gsi_04_12_raw_occupation_hash_is_insertion_order_independent() {
        let mut forward = Simulation::new();
        forward
            .substrate
            .raw_cell_occupation
            .mark_ground(2, 7, 0x04);
        forward.substrate.raw_cell_occupation.mark_deck(8, 3, 0x80);

        let mut reverse = Simulation::new();
        reverse.substrate.raw_cell_occupation.mark_deck(8, 3, 0x80);
        reverse
            .substrate
            .raw_cell_occupation
            .mark_ground(2, 7, 0x04);

        assert_eq!(forward.state_hash(), reverse.state_hash());
    }
}

#[cfg(all(test, tests_sim_world_t_z))]
mod overlay_grid_hash_tests {
    use super::Simulation;
    use crate::map::overlay::OverlayDataPack;
    use crate::sim::miner::ResourceType;
    use crate::sim::overlay_grid::OverlayGrid;

    #[test]
    fn gsi_04_09_empty_overlay_raw_data_changes_state_hash() {
        let mut sim_a = Simulation::new();
        let mut sim_b = Simulation::new();
        sim_a.overlay_grid = Some(OverlayGrid::from_overlay_packs(
            &[],
            &OverlayDataPack::from_cells([]),
            2,
            2,
        ));
        sim_b.overlay_grid = Some(OverlayGrid::from_overlay_packs(
            &[],
            &OverlayDataPack::from_cells([(1, 1, 42)]),
            2,
            2,
        ));

        let a = sim_a.overlay_grid.as_ref().expect("overlay grid A");
        let b = sim_b.overlay_grid.as_ref().expect("overlay grid B");
        assert_eq!(a.cell(1, 1).overlay_id, None);
        assert_eq!(b.cell(1, 1).overlay_id, None);
        assert_eq!(a.cell(1, 1).overlay_data, 0);
        assert_eq!(b.cell(1, 1).overlay_data, 42);
        assert!(a.iter_occupied().next().is_none());
        assert!(b.iter_occupied().next().is_none());
        assert_ne!(sim_a.state_hash(), sim_b.state_hash());
    }

    #[test]
    fn retained_wall_neighbor_plane_changes_hash_with_identical_final_cells() {
        let cells = vec![(-1, 0), (1, 2), (-1, 0), (-1, 0)];
        let mut sim_a = Simulation::new();
        let mut sim_b = Simulation::new();
        sim_a.overlay_grid = Some(OverlayGrid::from_finalized_map_payload(
            crate::map::authored_overlay::FinalizedOverlayPayload::from_cells_for_test(
                2,
                2,
                cells.clone(),
                vec![0, 1, 0, 0],
            ),
        ));
        sim_b.overlay_grid = Some(OverlayGrid::from_finalized_map_payload(
            crate::map::authored_overlay::FinalizedOverlayPayload::from_cells_for_test(
                2,
                2,
                cells,
                vec![0, 2, 0, 0],
            ),
        ));

        for y in 0..2 {
            for x in 0..2 {
                assert_eq!(
                    sim_a.overlay_grid.as_ref().expect("A").cell(x, y),
                    sim_b.overlay_grid.as_ref().expect("B").cell(x, y)
                );
            }
        }
        assert_ne!(sim_a.state_hash(), sim_b.state_hash());
    }

    #[test]
    fn a_new_grid_hashes_like_a_finalized_all_zero_wall_plane() {
        let mut fresh = Simulation::new();
        fresh.overlay_grid = Some(OverlayGrid::new(2, 2));

        let mut finalized = Simulation::new();
        finalized.overlay_grid = Some(OverlayGrid::from_finalized_map_payload(
            crate::map::authored_overlay::FinalizedOverlayPayload::from_cells_for_test(
                2,
                2,
                vec![(-1, 0); 4],
                vec![0; 4],
            ),
        ));

        assert_eq!(
            fresh.state_hash(),
            finalized.state_hash(),
            "every grid retains its wall plane, all-zero included"
        );
    }

    #[test]
    fn tiberium_cells_are_hashed_by_cell_not_placement_order() {
        use crate::sim::tiberium::test_support::place_tiberium;

        let mut forward = Simulation::new();
        place_tiberium(&mut forward, 8, 3, ResourceType::Gem, 3);
        place_tiberium(&mut forward, 2, 7, ResourceType::Ore, 3);

        let mut reverse = Simulation::new();
        place_tiberium(&mut reverse, 2, 7, ResourceType::Ore, 3);
        place_tiberium(&mut reverse, 8, 3, ResourceType::Gem, 3);
        assert_eq!(
            forward.state_hash(),
            reverse.state_hash(),
            "the overlay grid is folded in cell order, not placement order"
        );

        place_tiberium(&mut reverse, 8, 3, ResourceType::Gem, 4);
        assert_ne!(forward.state_hash(), reverse.state_hash());
        place_tiberium(&mut reverse, 8, 3, ResourceType::Ore, 3);
        assert_ne!(forward.state_hash(), reverse.state_hash());
    }
}

#[cfg(all(test, tests_sim_world_t_z))]
mod lifecycle_hash_tests {
    use super::Simulation;
    use crate::sim::game_entity::GameEntity;

    fn assert_entity_mutation_changes_hash(mutate: impl FnOnce(&mut GameEntity)) {
        let mut sim = Simulation::new();
        sim.substrate
            .entities
            .insert(GameEntity::test_default(1, "MTNK", "Americans", 5, 5));
        let before = sim.state_hash();
        mutate(sim.substrate.entities.get_mut(1).expect("fixture entity"));
        assert_ne!(before, sim.state_hash());
    }

    #[test]
    fn custom_foundation_geometry_changes_state_hash_despite_equal_names() {
        let _ares = crate::rules::ares_switch::read_ares_way(true);
        let rows = crate::rules::foundation::custom_tests::native_rows();
        let first = crate::rules::foundation::custom_tests::rules(&rows[0])
            .object("CUSTOM_TEST")
            .unwrap()
            .foundation
            .clone();
        let second = crate::rules::foundation::custom_tests::rules(&rows[1])
            .object("CUSTOM_TEST")
            .unwrap()
            .foundation
            .clone();
        assert_eq!(first.name(), second.name());
        let mut entity = GameEntity::test_default_of_category(
            1,
            "CUSTOM_TEST",
            "Americans",
            5,
            5,
            crate::map::entities::EntityCategory::Structure,
        );
        entity.seldom.type_facts.set_foundation_for_test(first);
        let mut sim = Simulation::new();
        sim.substrate.entities.insert(entity);
        let before = sim.state_hash();
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .seldom
            .type_facts
            .set_foundation_for_test(second);
        assert_ne!(before, sim.state_hash());
    }

    #[test]
    fn constructor_owner_distinguishes_otherwise_identical_transferred_entities() {
        use crate::map::entities::EntityCategory;
        use crate::sim::intern::test_intern;

        let hash_for = |initial: &str| {
            let mut sim = Simulation::new();
            sim.substrate
                .entities
                .insert(GameEntity::test_default_of_category(
                    1,
                    "GAPOWR",
                    initial,
                    5,
                    5,
                    EntityCategory::Structure,
                ));
            sim.substrate
                .entities
                .change_owner(1, test_intern("Russians"));
            sim.state_hash()
        };
        // All three objects now belong to Russia. Constructor ownership still
        // changes what a later capture back to America will do.
        let american = hash_for("Americans");
        let neutral = hash_for("Neutral");
        let russian = hash_for("Russians");
        assert_ne!(american, neutral);
        assert_ne!(american, russian);
        assert_ne!(neutral, russian);
    }

    #[test]
    fn lifecycle_authority_axes_and_pending_queue_change_state_hash() {
        {
            assert_entity_mutation_changes_hash(|entity| {
                entity
                    .lifecycle
                    .set_object_alive_for_test(!entity.lifecycle.object_alive());
            });
            assert_entity_mutation_changes_hash(|entity| {
                entity
                    .lifecycle
                    .set_in_limbo_for_test(!entity.lifecycle.in_limbo());
            });
            assert_entity_mutation_changes_hash(|entity| {
                entity
                    .lifecycle
                    .set_cell_marked_for_test(!entity.lifecycle.cell_marked());
            });
            assert_entity_mutation_changes_hash(|entity| {
                entity.death.set_dying_for_test(!entity.death.is_dying());
            });
            assert_entity_mutation_changes_hash(|entity| {
                let eligible = entity.seldom.removal_facts.dirty_rect_eligible();
                entity
                    .seldom
                    .removal_facts
                    .set_dirty_rect_eligible_for_test(!eligible);
            });
            assert_entity_mutation_changes_hash(|entity| {
                let recorded = entity.seldom.kill_record.is_recorded();
                entity
                    .seldom
                    .kill_record
                    .set_destruction_recorded_for_test(!recorded);
            });
            assert_entity_mutation_changes_hash(|entity| {
                let occupier = entity.seldom.removal_facts.occupier();
                entity.seldom.removal_facts.set_occupier_for_test(!occupier);
            });
            assert_entity_mutation_changes_hash(|entity| {
                entity.set_in_playfield(!entity.in_playfield());
            });
            assert_entity_mutation_changes_hash(|entity| {
                entity.passive_scan.timer_mut_for_test().arm(7, 12);
            });
        }
        {
            let mut empty = Simulation::new();
            let empty_hash = empty.state_hash();

            empty.substrate.pending_delete.queue_object(1);
            let one_hash = empty.state_hash();
            empty.substrate.pending_delete.queue_object(2);
            let ordered_hash = empty.state_hash();
            empty.substrate.pending_delete.swap_for_test(0, 1);
            let reversed_hash = empty.state_hash();

            assert_ne!(empty_hash, one_hash);
            assert_ne!(one_hash, ordered_hash);
            assert_ne!(ordered_hash, reversed_hash);
        }
    }
}

#[cfg(all(test, tests_sim_world_t_z))]
mod mission_authority_hash_tests {
    use super::Simulation;
    use crate::sim::combat::TargetKind;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::mission::leaf::MissionLeafState;
    use crate::sim::mission::state::MissionTestFixture;
    use crate::sim::mission::{MissionDispatchTimer, MissionId};

    fn hash_entity(entity: GameEntity) -> u64 {
        let mut sim = Simulation::new();
        sim.substrate.entities.insert(entity);
        sim.state_hash()
    }

    fn hash_mission(fixture: MissionTestFixture) -> u64 {
        let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
        entity.mission.apply_test_fixture(fixture);
        hash_entity(entity)
    }

    fn hash_leaf(leaf: MissionLeafState) -> u64 {
        let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
        entity.mission_leaf = leaf;
        hash_entity(entity)
    }

    fn hash_suspended_target(target: Option<TargetKind>) -> u64 {
        let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
        entity.seldom.suspended_attack_target.set_for_test(target);
        hash_entity(entity)
    }

    #[test]
    fn every_mission_com_raw_field_changes_state_hash() {
        let base = MissionTestFixture {
            current: MissionId::from_raw(0x1234_5678),
            suspended: MissionId::from_raw(i32::MIN),
            queued: MissionId::from_raw(i32::MAX),
            movement_bypass_latch: 0xa5,
            handler_state: 0x1122_3344,
            mission_start_frame: 0x5566_7788,
            ai_counter: 0x99aa_bbcc,
            dispatch_timer: MissionDispatchTimer::from_raw(-17, -29),
        };
        let base_hash = hash_mission(base);
        let variants = [
            (
                "current raw unknown selector",
                MissionTestFixture {
                    current: MissionId::from_raw(0x1234_5679),
                    ..base
                },
            ),
            (
                "suspended raw unknown selector",
                MissionTestFixture {
                    suspended: MissionId::from_raw(i32::MIN + 1),
                    ..base
                },
            ),
            (
                "queued raw unknown selector",
                MissionTestFixture {
                    queued: MissionId::from_raw(i32::MAX - 1),
                    ..base
                },
            ),
            (
                "movement bypass latch",
                MissionTestFixture {
                    movement_bypass_latch: 0xa4,
                    ..base
                },
            ),
            (
                "handler state",
                MissionTestFixture {
                    handler_state: 0x1122_3345,
                    ..base
                },
            ),
            (
                "mission start frame",
                MissionTestFixture {
                    mission_start_frame: 0x5566_7789,
                    ..base
                },
            ),
            (
                "AI counter",
                MissionTestFixture {
                    ai_counter: 0x99aa_bbcd,
                    ..base
                },
            ),
            (
                "dispatch timer start frame",
                MissionTestFixture {
                    dispatch_timer: MissionDispatchTimer::from_raw(-18, -29),
                    ..base
                },
            ),
            (
                "dispatch timer delay",
                MissionTestFixture {
                    dispatch_timer: MissionDispatchTimer::from_raw(-17, -30),
                    ..base
                },
            ),
        ];

        for (field, variant) in variants {
            assert_ne!(
                base_hash,
                hash_mission(variant),
                "{field} must contribute to the state hash"
            );
        }
    }

    #[test]
    fn every_class_mission_leaf_field_changes_state_hash() {
        {
            let base = MissionLeafState::unit_raw_for_test(1, 2, 3, 4);
            let base_hash = hash_leaf(base);
            for (field, variant) in [
                (
                    "deploy begin active",
                    MissionLeafState::unit_raw_for_test(5, 2, 3, 4),
                ),
                (
                    "deploy reverse active",
                    MissionLeafState::unit_raw_for_test(1, 5, 3, 4),
                ),
                (
                    "tracker byte 18",
                    MissionLeafState::unit_raw_for_test(1, 2, 5, 4),
                ),
                (
                    "tracker byte 19",
                    MissionLeafState::unit_raw_for_test(1, 2, 3, 5),
                ),
            ] {
                assert_ne!(
                    base_hash,
                    hash_leaf(variant),
                    "Unit {field} must contribute to the state hash"
                );
            }
        }
        {
            let base = MissionLeafState::infantry_raw_for_test(7, 12);
            let base_hash = hash_leaf(base);
            let mut water = base;
            water.set_infantry_water_state(false);
            let mut land = base;
            land.set_infantry_water_state(true);
            for (field, variant) in [
                (
                    "firing sequence latch",
                    MissionLeafState::infantry_raw_for_test(8, 12),
                ),
                ("Doing", MissionLeafState::infantry_raw_for_test(7, 13)),
                ("water state", water),
                ("land state", land),
            ] {
                assert_ne!(
                    base_hash,
                    hash_leaf(variant),
                    "Infantry {field} must contribute to the state hash"
                );
            }
            assert_ne!(hash_leaf(water), hash_leaf(land));
        }
        {
            let base = MissionLeafState::aircraft_raw_for_test(9, 10);
            let base_hash = hash_leaf(base);
            for (field, variant) in [
                (
                    "action latch",
                    MissionLeafState::aircraft_raw_for_test(10, 10),
                ),
                (
                    "transition ready latch",
                    MissionLeafState::aircraft_raw_for_test(9, 11),
                ),
            ] {
                assert_ne!(
                    base_hash,
                    hash_leaf(variant),
                    "Aircraft {field} must contribute to the state hash"
                );
            }
        }
        {
            assert_ne!(
                hash_leaf(MissionLeafState::building_raw_for_test(0)),
                hash_leaf(MissionLeafState::building_raw_for_test(1)),
                "Building ready latch must contribute to the state hash"
            );
        }
    }

    #[test]
    fn suspended_attack_target_presence_variant_and_payloads_change_state_hash() {
        let absent = hash_suspended_target(None);
        let entity_seven = hash_suspended_target(Some(TargetKind::Entity(7)));
        assert_ne!(
            absent, entity_seven,
            "suspended target presence must contribute to the state hash"
        );
        assert_ne!(
            entity_seven,
            hash_suspended_target(Some(TargetKind::Entity(8))),
            "suspended Entity target ID must contribute to the state hash"
        );

        let cell = hash_suspended_target(Some(TargetKind::Cell(10, 20)));
        assert_ne!(
            entity_seven, cell,
            "suspended target variant must contribute to the state hash"
        );
        assert_ne!(
            cell,
            hash_suspended_target(Some(TargetKind::Cell(11, 20))),
            "suspended Cell target X must contribute to the state hash"
        );
        assert_ne!(
            cell,
            hash_suspended_target(Some(TargetKind::Cell(10, 21))),
            "suspended Cell target Y must contribute to the state hash"
        );
    }

    #[test]
    fn falling_state_and_its_fall_rate_change_state_hash() {
        let base = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
        let mut falling = base.clone();
        falling.set_falling_down_for_test(true);
        let mut faster = falling.clone();
        faster.set_fall_rate_for_test(-2);

        let falling_hash = hash_entity(falling);
        assert_ne!(hash_entity(base), falling_hash, "IsFallingDown");
        assert_ne!(falling_hash, hash_entity(faster), "fall rate");
    }
}

#[cfg(all(test, tests_sim_world_t_z))]
mod track_authority_hash_tests {
    use super::Simulation;
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::components::{DriveCoord, TrackProgress};
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::locomotor::LocomotorState;
    use crate::sim::movement::track_process::TrackFamily;
    use crate::sim::movement::{DriveLocomotionRuntime, ShipLocomotionRuntime};

    fn supplied_track_world(ship: bool, active: bool) -> Simulation {
        let mut sim = Simulation::new();
        let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 10, 10);
        let track = if active {
            {
                let mut value = TrackProgress::default();
                value.set_turn_index(0);
                value.set_cursor(0);
                value
            }
        } else {
            TrackProgress::default()
        };
        let head_to = active.then_some(DriveCoord::cell(10, 9, 0));
        let mut loco = LocomotorState::for_test_kind(if ship {
            LocomotorKind::Ship
        } else {
            LocomotorKind::Drive
        });
        let family = TrackFamily::from_kind(loco.active_kind()).unwrap();
        loco.ensure_installed_track_state();
        loco.store_track_progress(family, track);
        loco.store_track_head(family, head_to);
        loco.store_track_valid(family, active);
        entity.locomotor = Some(loco);
        sim.substrate.entities.insert(entity);
        sim
    }

    #[test]
    fn current_hash_covers_each_retained_track_progress_field() {
        for (ship, suspended) in [(false, false), (false, true), (true, false), (true, true)] {
            for field in 0..6 {
                let mut sim = supplied_track_world(ship, true);
                if suspended {
                    assert!(
                        sim.substrate
                            .entities
                            .get_mut(1)
                            .unwrap()
                            .locomotor
                            .as_mut()
                            .unwrap()
                            .begin_piggyback(LocomotorKind::Teleport, 0)
                    );
                }
                let before = sim.state_hash();
                let entity = sim.substrate.entities.get_mut(1).unwrap();
                let family = if ship {
                    TrackFamily::Ship
                } else {
                    TrackFamily::Drive
                };
                let loco = entity.locomotor.as_mut().unwrap();
                if field == 5 {
                    loco.store_track_turn_latched(
                        family,
                        !loco.track_turn_latched(family).unwrap(),
                    );
                } else if field == 4 {
                    loco.store_track_valid(family, !loco.track_valid(family).unwrap());
                } else {
                    let mut track = loco.track_progress(family).unwrap();
                    match field {
                        0 => track.set_turn_index(1),
                        1 => track.set_cursor(1),
                        2 => track.set_reversed(true),
                        3 => track.set_residual(1),
                        _ => unreachable!(),
                    }
                    loco.store_track_progress(family, track);
                }
                assert_ne!(
                    before,
                    sim.state_hash(),
                    "ship={ship} suspended={suspended} field={field}"
                );
            }
        }
    }

    #[test]
    fn current_hash_covers_retained_coordinates_speed_and_permission() {
        let point = DriveCoord::cell(3, 4, 731);
        let drives = [
            DriveLocomotionRuntime::default().with_destination_for_test(Some(point)),
            DriveLocomotionRuntime::default().with_head_to_for_test(Some(point)),
            DriveLocomotionRuntime::default()
                .with_target_speed_fraction_for_test(crate::util::fixed_math::SIM_HALF),
            DriveLocomotionRuntime::default().with_end_permitted_for_test(false),
        ];
        let ships = [
            ShipLocomotionRuntime::default().with_destination_for_test(Some(point)),
            ShipLocomotionRuntime::default().with_head_to_for_test(Some(point)),
            ShipLocomotionRuntime::default()
                .with_target_speed_fraction_for_test(crate::util::fixed_math::SIM_HALF),
        ];
        for (ship, suspended) in [(false, false), (false, true), (true, false), (true, true)] {
            let base = supplied_track_world(ship, false);
            let mut empty = base;
            if suspended {
                assert!(
                    empty
                        .substrate
                        .entities
                        .get_mut(1)
                        .unwrap()
                        .locomotor
                        .as_mut()
                        .unwrap()
                        .begin_piggyback(LocomotorKind::Teleport, 0)
                );
            }
            let original = empty.state_hash();
            for field in 0..if ship { ships.len() } else { drives.len() } {
                let mut changed = Simulation::new();
                changed
                    .substrate
                    .entities
                    .insert(empty.substrate.entities.get(1).unwrap().clone());
                let loco = changed
                    .substrate
                    .entities
                    .get_mut(1)
                    .unwrap()
                    .locomotor
                    .as_mut()
                    .unwrap();
                assert!(if ship {
                    loco.install_ship_state_for_test(Some(ships[field].clone()))
                } else {
                    loco.install_drive_state_for_test(Some(drives[field].clone()))
                });
                assert_ne!(
                    original,
                    changed.state_hash(),
                    "ship={ship} suspended={suspended} retained field={field}"
                );
            }
            let loco = empty
                .substrate
                .entities
                .get_mut(1)
                .unwrap()
                .locomotor
                .as_mut()
                .unwrap();
            assert!(if ship {
                loco.install_ship_state_for_test(None)
            } else {
                loco.install_drive_state_for_test(None)
            });
            assert_ne!(
                original,
                empty.state_hash(),
                "absence must differ from a present default instance"
            );
        }
    }
}

#[cfg(all(test, tests_sim_world_t_z))]
mod state_hash_field_tests {
    use super::Simulation;
    use crate::sim::game_entity::GameEntity;

    #[test]
    fn drive_accelerates_changes_state_hash() {
        let mut sim_a = Simulation::new();
        let mut sim_b = Simulation::new();
        let entity_a = GameEntity::test_default(1, "GTNK", "Americans", 10, 10);
        let mut entity_b = entity_a.clone();
        entity_b.mover_facts.set_drive_accelerates_for_test(false);
        sim_a.substrate.entities.insert(entity_a);
        sim_b.substrate.entities.insert(entity_b);

        assert_ne!(sim_a.state_hash(), sim_b.state_hash());
    }

    #[test]
    fn house_fields_change_state_hash() {
        use crate::sim::house_state::{HouseDifficulty, HouseState};

        {
            let mut sim_a = Simulation::new();
            let mut sim_b = Simulation::new();
            let owner_a = sim_a.interner.intern("Computer1");
            let owner_b = sim_b.interner.intern("Computer1");
            assert_eq!(owner_a, owner_b);
            sim_a
                .houses
                .register(HouseState::new(owner_a, 0, None, false, 0, 10));
            let mut hard_house = HouseState::new(owner_b, 0, None, false, 0, 10);
            hard_house.set_difficulty_for_test(HouseDifficulty::Hard);
            sim_b.houses.register(hard_house);

            assert_ne!(sim_a.state_hash(), sim_b.state_hash());
        }
        {
            // Same stored difficulty, different bias: only the game-mode arm of
            // `set_difficulty` folds the country's ROF into `HouseClass+0x1A8`.
            let hashes: Vec<u64> = [
                crate::rules::rule_policy::RulePolicy::stock_campaign(),
                crate::rules::rule_policy::RulePolicy::stock_multiplayer(),
            ]
            .into_iter()
            .map(|policy| {
                let mut sim = Simulation::new();
                let owner = sim.interner.intern("Computer1");
                let mut house = HouseState::new(owner, 0, None, false, 0, 10);
                let mut general = crate::rules::ruleset::GeneralRules::default();
                general.difficulty_rows =
                    [0.8, 1.0, 1.2].map(|rof| crate::rules::ruleset::DifficultyRules {
                        rof,
                        ..Default::default()
                    });
                house.set_difficulty(
                    HouseDifficulty::Hard,
                    &general,
                    crate::rules::ruleset::CountryDifficultyBiases {
                        rof: crate::util::native_x87::NativeF64Bits::from_bits(0.9_f64.to_bits()),
                        ..Default::default()
                    },
                    policy,
                    0,
                    0,
                );
                sim.houses.register(house);
                sim.state_hash()
            })
            .collect();
            assert_ne!(hashes[0], hashes[1]);
        }
        {
            let mut baseline = Simulation::new();
            let mut changed = Simulation::new();
            let owner = baseline.interner.intern("Computer1");
            let changed_owner = changed.interner.intern("Computer1");
            assert_eq!(owner, changed_owner);
            let mut house = HouseState::new(owner, 0, None, false, 0, 10);
            house.set_base_center_for_test(Some((41, 52)));
            let mut changed_house = house.clone();
            changed_house.set_alternate_base_center_for_test((93, 106));
            baseline.houses.register(house);
            changed.houses.register(changed_house);

            assert_ne!(baseline.state_hash(), changed.state_hash());
            assert_eq!(
                baseline.houses[&owner].base_center(),
                changed.houses[&owner].base_center()
            );
        }
        {
            let mut sim_a = Simulation::new();
            let mut sim_b = Simulation::new();
            let owner_a = sim_a.interner.intern("Computer1");
            let owner_b = sim_b.interner.intern("Computer1");
            assert_eq!(owner_a, owner_b);
            sim_a
                .houses
                .register(HouseState::new(owner_a, 0, None, false, 0, 10));
            let mut changed = HouseState::new(owner_b, 0, None, false, 0, 10);
            changed.base_reservation_mut().update_bounds(3, 4, 5, 6);
            changed
                .base_reservation_mut()
                .append_perimeter_cell_if_absent(u32::from(3u16) | (u32::from(4u16) << 16));
            sim_b.houses.register(changed);

            assert_ne!(sim_a.state_hash(), sim_b.state_hash());
        }
        {
            fn fixture() -> (Simulation, crate::sim::intern::InternedId) {
                let mut sim = Simulation::new();
                let owner = sim.interner.intern("Computer1");
                sim.houses
                    .register(HouseState::new(owner, 0, None, false, 0, 10));
                (sim, owner)
            }

            let (baseline, _) = fixture();
            let baseline_hash = baseline.state_hash();

            let (mut mode, owner) = fixture();
            mode.houses
                .get_mut(&owner)
                .unwrap()
                .strategy_emergency_mut()
                .set_state_four();
            assert_ne!(baseline_hash, mode.state_hash(), "mode is hashed");

            let (mut bias, owner) = fixture();
            bias.houses
                .get_mut(&owner)
                .unwrap()
                .strategy_emergency_mut()
                .set_all_to_hunt_bias();
            assert_ne!(baseline_hash, bias.state_hash(), "bias latch is hashed");

            let (mut attack_frame, owner) = fixture();
            attack_frame
                .houses
                .get_mut(&owner)
                .unwrap()
                .strategy_emergency_mut()
                .note_building_attack(-17);
            assert_ne!(
                baseline_hash,
                attack_frame.state_hash(),
                "last Building attack frame is hashed"
            );

            let (mut attacker_index, owner) = fixture();
            attacker_index
                .houses
                .get_mut(&owner)
                .unwrap()
                .strategy_emergency_mut()
                .note_building_attacker(2);
            assert_ne!(
                baseline_hash,
                attacker_index.state_hash(),
                "last attacker House index is hashed"
            );

            let (mut timer, owner) = fixture();
            timer
                .houses
                .get_mut(&owner)
                .unwrap()
                .set_strategy_timer_for_test(crate::sim::timer::CdTimer::started(12, 106));
            assert_ne!(
                baseline_hash,
                timer.state_hash(),
                "Strategy timer is hashed"
            );
        }
        {
            let mut north = Simulation::new();
            let mut south = Simulation::new();
            let north_owner = north.interner.intern("Player");
            let south_owner = south.interner.intern("Player");
            assert_eq!(north_owner, south_owner);

            let mut north_house = HouseState::new(north_owner, 0, None, true, 0, 10);
            north_house.set_waypoint_edge_for_test(0);
            north.houses.register(north_house);

            let mut south_house = HouseState::new(south_owner, 0, None, true, 0, 10);
            south_house.set_waypoint_edge_for_test(2);
            south.houses.register(south_house);

            assert_ne!(north.state_hash(), south.state_hash());
        }
    }

    #[test]
    fn gsi_04_05_base_plan_state_and_entity_facts_are_current_schema_hash_authority() {
        use crate::sim::base_plan::{BasePlanNode, pack_base_plan_cell};
        use crate::sim::house_state::HouseState;

        fn house_sim(nodes: Vec<BasePlanNode>, percent_built: i32) -> Simulation {
            let mut sim = Simulation::new();
            let owner = sim.interner.intern("Computer1");
            let mut house = HouseState::new(owner, 0, None, false, 0, 10);
            house
                .base_plan_mut()
                .set_percent_built_for_test(percent_built);
            house.base_plan_mut().set_nodes_for_test(nodes);
            sim.houses.register(house);
            sim
        }

        let first = BasePlanNode {
            type_or_control: 4,
            packed_cell: pack_base_plan_cell(7, 8),
            filled: false,
            retry_count: 2,
        };
        let second = BasePlanNode {
            type_or_control: -3,
            packed_cell: pack_base_plan_cell(-1, 5),
            filled: true,
            retry_count: -9,
        };
        let baseline = house_sim(vec![first, second], 50);
        let reversed = house_sim(vec![second, first], 50);
        assert_ne!(baseline.state_hash(), reversed.state_hash());

        for changed in [
            house_sim(vec![first, second], 51),
            house_sim(
                vec![
                    BasePlanNode {
                        type_or_control: 5,
                        ..first
                    },
                    second,
                ],
                50,
            ),
            house_sim(
                vec![
                    BasePlanNode {
                        packed_cell: pack_base_plan_cell(9, 8),
                        ..first
                    },
                    second,
                ],
                50,
            ),
            house_sim(
                vec![
                    BasePlanNode {
                        filled: true,
                        ..first
                    },
                    second,
                ],
                50,
            ),
            house_sim(
                vec![
                    BasePlanNode {
                        retry_count: 3,
                        ..first
                    },
                    second,
                ],
                50,
            ),
        ] {
            assert_ne!(baseline.state_hash(), changed.state_hash());
        }

        let mut entity_a = Simulation::new();
        let mut entity_b = Simulation::new();
        let mut a = GameEntity::test_default(1, "GAPOWR", "Computer1", 10, 10);
        let mut b = a.clone();
        a.seldom.type_facts.set_base_plan_type_index_for_test(2);
        a.seldom.type_facts.set_base_plan_is_defense_for_test(true);
        a.seldom
            .type_facts
            .set_base_plan_has_undeploy_target_for_test(true);
        b.seldom.type_facts.set_base_plan_type_index_for_test(3);
        entity_a.substrate.entities.insert(a);
        entity_b.substrate.entities.insert(b);
        assert_ne!(entity_a.state_hash(), entity_b.state_hash());
    }

    #[test]
    fn house_ai_activation_hash_matches_native_direct_crc_fields_only() {
        use crate::sim::house_state::{HouseAiActivationLatches, HouseState};

        fn fixture(latches: HouseAiActivationLatches) -> Simulation {
            let mut sim = Simulation::new();
            let owner = sim.interner.intern("Computer1");
            let mut house = HouseState::new(owner, 0, None, false, 0, 10);
            house.set_ai_activation_for_test(latches);
            sim.houses.register(house);
            sim
        }

        let baseline = fixture(HouseAiActivationLatches::default());
        let production = fixture(HouseAiActivationLatches {
            production: true,
            autocreate_allowed: false,
            ai_triggers_active: false,
            auto_base_building: false,
        });
        let autocreate = fixture(HouseAiActivationLatches {
            production: false,
            autocreate_allowed: true,
            ai_triggers_active: false,
            auto_base_building: false,
        });
        let ai_triggers = fixture(HouseAiActivationLatches {
            production: false,
            autocreate_allowed: false,
            ai_triggers_active: true,
            auto_base_building: false,
        });
        let auto_base = fixture(HouseAiActivationLatches {
            production: false,
            autocreate_allowed: false,
            ai_triggers_active: false,
            auto_base_building: true,
        });

        assert_ne!(baseline.state_hash(), production.state_hash());
        assert_ne!(baseline.state_hash(), autocreate.state_hash());
        assert_ne!(baseline.state_hash(), ai_triggers.state_hash());
        assert_eq!(baseline.state_hash(), auto_base.state_hash());
    }

    #[test]
    fn gsi_04_05_techno_base_defense_state_changes_world_hash() {
        let mut baseline = Simulation::new();
        let entity = crate::sim::game_entity::GameEntity::test_default(1, "E1", "Computer1", 3, 4);
        baseline.substrate.entities.insert(entity.clone());
        let baseline_hash = baseline.state_hash();

        let mut changed = Simulation::new();
        let mut entity = entity;
        entity
            .seldom
            .base_defense_response
            .set_recruitable_b_for_test(false);
        entity.set_archive_target(Some(crate::sim::combat::TargetKind::Entity(7)));
        entity
            .seldom
            .base_defense_response
            .set_cooldown_for_test(crate::sim::timer::CdTimer::started(12, 225));
        changed.substrate.entities.insert(entity);
        assert_ne!(baseline_hash, changed.state_hash());
    }
}

#[cfg(all(test, tests_sim_world_t_z))]
mod particle_hash_tests {
    use super::Simulation;
    use crate::rules::particle_system_type::ParticleSystemTypeId;
    use crate::rules::particle_type::ParticleTypeId;
    use crate::sim::particles::{Particle, ParticleSystem};
    use crate::util::native_x87::{NativeF32Bits, NativeF64Bits};
    use glam::IVec3;

    fn fake_system(coords: IVec3) -> ParticleSystem {
        ParticleSystem::test_fixture(0, ParticleSystemTypeId(0), coords)
    }

    fn insert_system(sim: &mut Simulation, mut system: ParticleSystem) -> u64 {
        let id = sim.substrate.allocate_stable_id();
        system.set_stable_id_for_test(id);
        sim.substrate.particle_systems.insert(system);
        sim.reveal_particle_system(id);
        id
    }

    fn test_particle() -> Particle {
        let mut particle = Particle::test_fixture(ParticleTypeId(0), IVec3::new(-1, 2, 3));
        particle.set_origin_for_test(IVec3::ZERO);
        particle.set_lifetime_remaining_for_test(9);
        particle
    }

    fn hash_with_particle(particle: Particle) -> u64 {
        let mut sim = Simulation::new();
        let mut system = fake_system(IVec3::ZERO);
        system.particles_mut_for_test().push(particle);
        insert_system(&mut sim, system);
        sim.state_hash()
    }

    #[test]
    fn common_particle_color_and_position_fields_change_the_state_hash() {
        let base = test_particle();
        let base_hash = hash_with_particle(base.clone());
        let edits: [fn(&mut Particle); 5] = [
            |p| p.set_color_for_test([80, 255, 255], 0, NativeF64Bits::POSITIVE_ZERO),
            |p| p.set_color_for_test([0; 3], -1, NativeF64Bits::POSITIVE_ZERO),
            |p| p.set_color_for_test([0; 3], 0, NativeF64Bits::NEGATIVE_ZERO),
            |p| p.float_position_mut_for_test()[0] = NativeF32Bits::NEGATIVE_ZERO,
            |p| p.set_target_for_test(IVec3::ONE),
        ];
        for edit in edits {
            let mut changed = base.clone();
            edit(&mut changed);
            assert_ne!(base_hash, hash_with_particle(changed));
        }
    }

    fn hash_with_system(system: ParticleSystem) -> u64 {
        let mut sim = Simulation::new();
        insert_system(&mut sim, system);
        sim.state_hash()
    }

    /// Every saved system and particle field reaches the hash (#855).
    #[test]
    fn saved_system_and_particle_fields_change_the_state_hash() {
        let mut base = fake_system(IVec3::ZERO);
        base.particles_mut_for_test().push(test_particle());
        let base_hash = hash_with_system(base.clone());
        let owner_house = Simulation::new().interner.intern("Americans");
        type Edit = Box<dyn Fn(&mut ParticleSystem)>;
        let edits: Vec<Edit> = vec![
            Box::new(|s| s.set_offset_for_test(IVec3::new(1, 0, 0))),
            Box::new(|s| {
                s.set_spawn_timer_for_test(crate::util::native_x87::NativeF32Bits::from_bits(
                    0x4000_0000,
                ))
            }),
            Box::new(|s| s.set_spark_spawn_frames_for_test(1)),
            Box::new(|s| {
                s.set_target_ref_for_test(Some(crate::sim::combat::TargetKind::Entity(7)))
            }),
            Box::new(|s| s.set_owner_entity_for_test(Some(7))),
            Box::new(|s| s.set_target_coords_for_test(IVec3::new(0, 1, 0))),
            Box::new(move |s| s.set_owner_house_for_test(Some(owner_house))),
            Box::new(|s| s.particles_mut_for_test()[0].set_origin_for_test(IVec3::new(0, 0, 1))),
            Box::new(|s| {
                s.particles_mut_for_test()[0].direction_mut_for_test()[1] =
                    crate::util::native_x87::NativeF32Bits::ONE
            }),
            Box::new(|s| {
                s.particles_mut_for_test()[0].motion_mut_for_test()[1] =
                    crate::util::native_x87::NativeF32Bits::ONE
            }),
            Box::new(|s| {
                s.particles_mut_for_test()[0]
                    .set_velocity_for_test(crate::util::native_x87::NativeF32Bits::ONE)
            }),
            Box::new(|s| s.particles_mut_for_test()[0].set_damage_counter_for_test(1)),
            Box::new(|s| s.particles_mut_for_test()[0].set_state_ai_advance_for_test(1)),
            Box::new(|s| s.particles_mut_for_test()[0].set_drift_x_for_test(1)),
            Box::new(|s| s.particles_mut_for_test()[0].set_drift_y_for_test(1)),
            Box::new(|s| s.particles_mut_for_test()[0].set_drift_z_for_test(1)),
            Box::new(|s| s.particles_mut_for_test()[0].prev_delta_mut_for_test()[2] = 1),
        ];
        for (index, edit) in edits.iter().enumerate() {
            let mut system = base.clone();
            edit(&mut system);
            assert_ne!(base_hash, hash_with_system(system), "edit {index}");
        }
    }

    #[test]
    fn terrain_spawner_active_fields_change_state_hash() {
        use crate::sim::terrain_spawn::TerrainAnimationState;

        let mut sim_a = Simulation::new();
        let mut sim_b = Simulation::new();
        let type_ref = sim_a.interner.intern("TIBTRE01");
        let state = TerrainAnimationState::new(
            type_ref,
            crate::util::native_x87::NativeF32Bits::ONE,
            3,
            22,
            0,
        );
        sim_a
            .terrain_objects
            .animations_mut_for_test()
            .insert(10, state.clone());
        sim_b
            .terrain_objects
            .animations_mut_for_test()
            .insert(10, state);
        assert_eq!(sim_a.state_hash(), sim_b.state_hash());

        let spawner_b = sim_b
            .terrain_objects
            .animations_mut_for_test()
            .get_mut(&10)
            .unwrap();
        spawner_b.advance_for_test(0, &mut crate::sim::rng::SimRng::new(0));
        assert_ne!(
            sim_a.state_hash(),
            sim_b.state_hash(),
            "all terrain spawner state fields must affect state hash",
        );
    }
}

#[cfg(all(test, tests_sim_world_t_z))]
mod radio_contact_hash_tests {
    use super::Simulation;
    use crate::map::entities::EntityCategory;
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;

    fn vehicle_entity(sim: &mut Simulation, id: u64) -> GameEntity {
        GameEntity::new_at_frame_zero_for_test(
            id,
            10,
            10,
            0,
            0,
            sim.interner.intern("Americans"),
            Health::new(100),
            sim.interner.intern("MTNK"),
            EntityCategory::Unit,
            0,
            5,
            true,
        )
    }

    #[test]
    fn live_radio_contacts_change_state_hash_per_mover() {
        let mut sim_a = Simulation::new();
        let mut sim_b = Simulation::new();
        let mut contacted = vehicle_entity(&mut sim_a, 1);
        let unrelated = vehicle_entity(&mut sim_a, 2);
        let contacted_b = vehicle_entity(&mut sim_b, 1);
        let unrelated_b = vehicle_entity(&mut sim_b, 2);

        contacted.mark_live_contact_with(100);
        sim_a.substrate.entities.insert(contacted);
        sim_a.substrate.entities.insert(unrelated);
        sim_b.substrate.entities.insert(contacted_b);
        sim_b.substrate.entities.insert(unrelated_b);

        assert_ne!(
            sim_a.state_hash(),
            sim_b.state_hash(),
            "per-mover live contacts must affect deterministic state hash",
        );
        assert!(
            !sim_a
                .substrate
                .entities
                .get(2)
                .unwrap()
                .has_live_contact_with(100)
        );
    }

    #[test]
    fn despawn_contact_cleanup_hash_matches_never_contacted_state() {
        let mut with_stale_contact = Simulation::new();
        let mut never_contacted = Simulation::new();

        let mut removed = vehicle_entity(&mut with_stale_contact, 1);
        let mut survivor = vehicle_entity(&mut with_stale_contact, 2);
        removed.mark_live_contact_with(2);
        survivor.mark_live_contact_with(1);
        with_stale_contact.substrate.entities.insert(removed);
        with_stale_contact.substrate.entities.insert(survivor);

        let removed_b = vehicle_entity(&mut never_contacted, 1);
        let survivor_b = vehicle_entity(&mut never_contacted, 2);
        never_contacted.substrate.entities.insert(removed_b);
        never_contacted.substrate.entities.insert(survivor_b);

        for id in [1, 2] {
            assert!(matches!(
                with_stale_contact.reveal(id),
                crate::sim::world::RevealOutcome::Revealed { .. }
            ));
            assert!(matches!(
                never_contacted.reveal(id),
                crate::sim::world::RevealOutcome::Revealed { .. }
            ));
        }

        with_stale_contact.despawn_entity(1);
        never_contacted.despawn_entity(1);

        assert_eq!(
            with_stale_contact.state_hash(),
            never_contacted.state_hash(),
            "cleanup should leave the same hash as a sim that never carried the stale contact",
        );
    }
}

#[cfg(all(test, tests_sim_world_t_z))]
mod building_anim_slot_hash_tests {
    #[test]
    fn retained_slot_identity_flag_and_runtime_change_hash() {
        let (mut sim, rules, id) = crate::sim::building_art::slot_test_fixture();
        let empty = sim.state_hash();
        let anim = sim
            .set_building_anim_slot(id, 3, false, false, 0, &rules)
            .unwrap();
        let present = sim.state_hash();
        assert_ne!(empty, present);
        sim.substrate
            .anims
            .get_mut(anim)
            .unwrap()
            .runtime_mut_for_test()
            .current_frame += 1;
        assert_ne!(present, sim.state_hash());
        let advanced = sim.state_hash();
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .building_art
            .set_damage_state_active_for_test(true);
        assert_ne!(advanced, sim.state_hash());
        let retained = sim.state_hash();
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .building_art
            .anim_slots_mut_for_test()
            .swap(3, 4);
        assert_ne!(retained, sim.state_hash());
    }
}

#[cfg(all(test, tests_sim_world_t_z))]
mod infantry_hash_tests {
    use super::Simulation;
    use crate::map::entities::EntityCategory;
    use crate::sim::animation::{Animation, SequenceKind};
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::infantry::InfantryRuntime;

    fn hash_entity(sim: &mut Simulation, category: EntityCategory) -> GameEntity {
        GameEntity::new_at_frame_zero_for_test(
            1,
            0,
            0,
            0,
            0,
            sim.interner.intern("Allies"),
            Health::new(100),
            sim.interner.intern("E1"),
            category,
            0,
            5,
            false,
        )
    }

    fn hash_with_animation(category: EntityCategory, animation: Option<Animation>) -> u64 {
        let mut sim = Simulation::new();
        let mut entity = hash_entity(&mut sim, category);
        entity.animation = animation;
        sim.substrate.entities.insert(entity);
        sim.state_hash()
    }

    #[test]
    fn every_gameplay_read_animation_field_changes_state_hash() {
        let hash = |animation| hash_with_animation(EntityCategory::Unit, animation);
        let absent = hash(None);
        let base = Animation::new(SequenceKind::Stand);
        let base_hash = hash(Some(base.clone()));
        assert_ne!(absent, base_hash);

        let mut sequence = base.clone();
        sequence.set_sequence_for_test(SequenceKind::Attack);
        assert_ne!(base_hash, hash(Some(sequence)));

        let mut frame_index = base.clone();
        frame_index.set_frame_index_for_test(1);
        assert_ne!(base_hash, hash(Some(frame_index)));

        let mut elapsed_frames = base.clone();
        elapsed_frames.set_elapsed_frames_for_test(1);
        assert_ne!(base_hash, hash(Some(elapsed_frames)));

        let mut finished = base;
        finished.set_finished_for_test(true);
        assert_ne!(base_hash, hash(Some(finished)));
    }

    #[test]
    fn infantry_presentation_animation_cannot_change_simulation_hash() {
        // The class owns Doing/Stage. A retained presentation component after
        // restore cannot become a second simulation clock through hashing.
        let absent = hash_with_animation(EntityCategory::Infantry, None);
        let mut animation = Animation::new(SequenceKind::Die1);
        animation.set_frame_index_for_test(u16::MAX);
        animation.set_elapsed_frames_for_test(u16::MAX);
        animation.set_finished_for_test(true);
        assert_eq!(
            absent,
            hash_with_animation(EntityCategory::Infantry, Some(animation))
        );
    }

    #[test]
    fn infantry_fear_prone_and_cell_entry_blocked_change_hash() {
        {
            let mut sim_a = Simulation::new();
            let mut sim_b = Simulation::new();
            let mut a = hash_entity(&mut sim_a, EntityCategory::Infantry);
            let b = hash_entity(&mut sim_b, EntityCategory::Infantry);
            a.infantry = Some({
                let mut runtime = InfantryRuntime::new();
                runtime.set_fear_level_for_test(10);
                runtime.set_is_prone_for_test(false);
                runtime
            });
            sim_a.substrate.entities.insert(a);
            sim_b.substrate.entities.insert(b);
            assert_ne!(sim_a.state_hash(), sim_b.state_hash());

            let mut sim_a = Simulation::new();
            let mut sim_b = Simulation::new();
            let mut a = hash_entity(&mut sim_a, EntityCategory::Infantry);
            let b = hash_entity(&mut sim_b, EntityCategory::Infantry);
            a.infantry = Some({
                let mut runtime = InfantryRuntime::new();
                runtime.set_fear_level_for_test(0);
                runtime.set_is_prone_for_test(true);
                runtime
            });
            sim_a.substrate.entities.insert(a);
            sim_b.substrate.entities.insert(b);
            assert_ne!(sim_a.state_hash(), sim_b.state_hash());
        }
        {
            let mut sim_a = Simulation::new();
            let mut sim_b = Simulation::new();
            let mut a = hash_entity(&mut sim_a, EntityCategory::Infantry);
            let b = hash_entity(&mut sim_b, EntityCategory::Infantry);
            a.infantry = Some({
                let mut runtime = InfantryRuntime::new();
                runtime.set_cell_entry_blocked_for_test(true);
                runtime
            });
            sim_a.substrate.entities.insert(a);
            sim_b.substrate.entities.insert(b);
            assert_ne!(sim_a.state_hash(), sim_b.state_hash());
            // A clear byte folds nothing: prior streams keep their pins.
            let mut sim_c = Simulation::new();
            let c = hash_entity(&mut sim_c, EntityCategory::Infantry);
            sim_c.substrate.entities.insert(c);
            assert_eq!(sim_b.state_hash(), sim_c.state_hash());
        }
    }

    #[test]
    fn foot_path_runtime_hash_and_snapshot_survive_without_movement_adapter() {
        use crate::sim::components::FootPathRuntime;
        use crate::sim::timer::CdTimer;
        let mut sim = Simulation::new();
        let actor = hash_entity(&mut sim, EntityCategory::Infantry);
        sim.substrate.entities.insert(actor);
        let before = sim.state_hash();
        let mut retained = FootPathRuntime::at_frame(0);
        retained.set_movement_timer_for_test(CdTimer::from_raw(-1, -7));
        retained.set_blocked_timer_for_test(CdTimer::from_raw(i32::MAX - 2, 31));
        retained.set_path_blocked_for_test(true);
        retained.set_retries_left_for_test(u32::MAX);
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .navigation
            .set_path_runtime_for_test(retained);
        assert_ne!(sim.state_hash(), before);
        let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "foot-path", 0);
        let loaded = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .unwrap()
            .sim;
        let actor = loaded.entities().get(1).unwrap();
        assert!(crate::sim::movement::motion_query::is_moving(&actor) != Some(true));
        assert_eq!(actor.navigation.path_runtime().clone(), retained);
    }

    #[test]
    fn foot_retarget_snapshot_retains_native_next_scan_mask_and_empty_clear() {
        let native =
            crate::test_fixture::json("tools/spatial_oracle/anytown_damage/foot_missions.json");
        let row = native["greatest_threat_rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["input"]["name"] == "MTNK_concrete_mask0_latch1_MTNK_live0")
            .unwrap();
        let input_mask = row["greatest_threat_masks"]["foot_greatest"][0]
            .as_u64()
            .unwrap() as u32;
        let effective_mask = row["greatest_threat_masks"]["greatest"][0]
            .as_u64()
            .unwrap() as u32;
        assert_eq!(row["before"]["scan"], 1);
        assert_eq!(row["returned_eax"], 0);
        assert_eq!(row["after"]["scan"], 0);

        let mut sim = Simulation::new();
        // Native Scenario load reseeds its stream to zero. Isolate this
        // retained Foot state from that established load-time RNG change.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let actor = hash_entity(&mut sim, EntityCategory::Infantry);
        sim.substrate.entities.insert(actor);
        let clear_hash = sim.state_hash();
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .seldom
            .foot_retarget_after_stop
            .mark_stopped_cannot_fire();
        let retained_hash = sim.state_hash();
        assert_ne!(retained_hash, clear_hash);
        let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "foot-retarget", 0);
        let mut loaded = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .unwrap()
            .sim;
        assert_eq!(loaded.state_hash(), retained_hash);
        let actor = loaded.substrate.entities.get_mut(1).unwrap();
        assert!(actor.foot_retarget_after_stop());
        assert_eq!(
            actor
                .seldom
                .foot_retarget_after_stop
                .coerce_mask(input_mask),
            effective_mask
        );
        actor.seldom.foot_retarget_after_stop.finish_scan(false);
        assert!(!actor.foot_retarget_after_stop());
        assert_eq!(
            actor
                .seldom
                .foot_retarget_after_stop
                .coerce_mask(input_mask),
            input_mask
        );
        assert_eq!(loaded.state_hash(), clear_hash);
    }

    #[test]
    fn inherited_foot_firing_state_survives_snapshot_and_changes_noninfantry_hash() {
        let native =
            crate::test_fixture::json("tools/spatial_oracle/anytown_damage/foot_missions.json");
        let raw = native["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["input"]["name"] == "MTNK_area_guard_leash_firing_bypass")
            .unwrap()["before"]["firing"]
            .as_u64()
            .unwrap() as u8;
        // The Unit handler control supplies this byte. Aircraft also inherits
        // the raw persisted Foot field; its save/load assertion is a Rust
        // storage regression, not an Aircraft firing-producer parity claim.
        for (category, type_name) in [
            (EntityCategory::Unit, "MTNK"),
            (EntityCategory::Aircraft, "ORCA"),
        ] {
            let mut sim = Simulation::new();
            sim.scenario_rng = crate::sim::rng::SimRng::new(0);
            let owner = sim.interner.intern("Allies");
            let type_ref = sim.interner.intern(type_name);
            sim.substrate
                .entities
                .insert(GameEntity::new_at_frame_zero_for_test(
                    1,
                    0,
                    0,
                    0,
                    0,
                    owner,
                    Health::new(100),
                    type_ref,
                    category,
                    0,
                    5,
                    false,
                ));
            let clear_hash = sim.state_hash();
            sim.substrate
                .entities
                .get_mut(1)
                .unwrap()
                .mission_leaf
                .set_foot_firing_sequence(raw);
            let retained_hash = sim.state_hash();
            assert_ne!(retained_hash, clear_hash, "{type_name}");
            let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, type_name, 0);
            let mut loaded = crate::sim::snapshot::GameSnapshot::load(&bytes)
                .unwrap()
                .sim;
            assert_eq!(loaded.state_hash(), retained_hash, "{type_name}");
            assert_eq!(
                loaded
                    .entities()
                    .get(1)
                    .unwrap()
                    .mission_leaf
                    .foot_firing_sequence_latch(),
                raw,
                "{type_name}"
            );
            loaded
                .substrate
                .entities
                .get_mut(1)
                .unwrap()
                .mission_leaf
                .set_foot_firing_sequence(0);
            assert_eq!(loaded.state_hash(), clear_hash, "{type_name}");
        }
    }

    #[test]
    fn foot_scold_byte_survives_snapshot_and_changes_the_hash() {
        // Original raw Load and no-init Foot construction preserve 0, 1 and
        // 255 separately; the guard only distinguishes zero from nonzero.
        // Native comparison: tools/spatial_oracle/foot_scold_latch.json.
        let mut sim = Simulation::new();
        // Scenario deserialization deliberately reseeds to0. Start this
        // retained-byte fixture at that same state so its full-hash assertion
        // isolates persistence, not the intentionally changed RNG future.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let actor = hash_entity(&mut sim, EntityCategory::Infantry);
        assert_eq!(actor.navigation.path_runtime().scold_latch_raw(), 0);
        sim.substrate.entities.insert(actor);
        let clear_hash = sim.state_hash();
        let mut retained_hashes = vec![clear_hash];
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/foot_scold_latch.json",
        ))
        .unwrap();
        for row in native["imported_latch"].as_array().unwrap() {
            let raw = row["supplied_saved_byte"].as_u64().unwrap() as u8;
            sim.substrate
                .entities
                .get_mut(1)
                .unwrap()
                .navigation
                .path_runtime_mut()
                .set_scold_latch_for_test(raw);
            let current_hash = sim.state_hash();
            assert!(!retained_hashes.contains(&current_hash) || raw == 0);
            retained_hashes.push(current_hash);

            let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "scold", 0);
            let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
                .unwrap()
                .sim;
            let actor = restored.substrate.entities.get(1).unwrap();
            assert!(crate::sim::movement::motion_query::is_moving(&actor) != Some(true));
            assert_eq!(
                u64::from(actor.navigation.path_runtime().scold_latch_raw()),
                row["after_original_noinit_constructor"].as_u64().unwrap()
            );
            assert_eq!(restored.state_hash(), current_hash);
            let path = restored
                .substrate
                .entities
                .get_mut(1)
                .unwrap()
                .navigation
                .path_runtime_mut();
            assert_eq!(path.clear_scold_latch(), raw != 0);
            assert_eq!(path.scold_latch_raw(), 0);
            assert!(!path.clear_scold_latch(), "a consumed latch stays clear");
            assert_eq!(restored.state_hash(), clear_hash);
        }
    }
}

#[cfg(all(test, tests_sim_world_t_z))]
mod smudge_hash_tests {
    use super::*;
    use crate::sim::smudge_grid::{SmudgeCell, SmudgeGrid};

    #[test]
    fn hash_changes_when_smudge_placed() {
        let mut sim = Simulation::new();
        sim.smudge_grid = Some(SmudgeGrid::new(8, 8));
        let h0 = sim.state_hash();
        if let Some(grid) = sim.smudge_grid.as_mut() {
            grid.test_force_set(
                2,
                3,
                SmudgeCell {
                    type_id: Some(0),
                    frame_offset: 0,
                },
            );
        }
        let h1 = sim.state_hash();
        assert_ne!(h0, h1);
    }
}

#[cfg(all(test, tests_sim_world_t_z))]
mod bridge_state_hash_tests {
    use super::Simulation;
    use crate::sim::bridge_state::{BridgeEndpointRecord, BridgeRecordKind, BridgeRuntimeState};

    #[test]
    fn bridge_endpoint_record_kind_difference_changes_state_hash() {
        let mut sim_a = Simulation::new();
        let mut sim_b = Simulation::new();

        let mut state_a = BridgeRuntimeState::default();
        let mut state_b = BridgeRuntimeState::default();
        let mut record = BridgeEndpointRecord {
            endpoint_a: (1, 1),
            endpoint_b: (4, 1),
            active: true,
            bridge_kind: BridgeRecordKind::High,
        };
        state_a.test_set_endpoint_records(vec![record]);
        record.bridge_kind = BridgeRecordKind::Low;
        state_b.test_set_endpoint_records(vec![record]);

        sim_a.bridge_state = Some(state_a);
        sim_b.bridge_state = Some(state_b);

        assert_ne!(
            sim_a.state_hash(),
            sim_b.state_hash(),
            "bridge endpoint record kind must contribute to state hash",
        );
    }

    #[test]
    fn one_advance_is_one_wrapping_native_frame_for_any_host_duration() {
        {
            let mut sim = Simulation::new();
            let host_durations = [0, 1, 22, 67, 1_000, u32::MAX];
            for (index, tick_ms) in host_durations.into_iter().enumerate() {
                sim.advance_tick(&[], None, None, None, tick_ms);
                assert_eq!(sim.session.binary_frame(), index as u32 + 1);
            }
        }
        {
            let mut sim = Simulation::new();
            sim.session.set_binary_frame_for_test(u32::MAX);

            sim.advance_tick(&[], None, None, None, 22);

            assert_eq!(sim.session.binary_frame(), 0);
        }
    }
}

#[cfg(all(test, tests_sim_world_t_z))]
mod native_frame_tests {
    use super::Simulation;

    #[test]
    fn native_frame_changes_state_hash() {
        let mut sim_a = Simulation::new();
        let sim_b = Simulation::new();
        sim_a.advance_tick(&[], None, None, None, 22);
        assert_ne!(sim_a.state_hash(), sim_b.state_hash());
    }

    #[test]
    fn diagnostic_total_sim_ms_does_not_change_state_hash() {
        let mut sim_a = Simulation::new();
        let sim_b = Simulation::new();
        sim_a.session.set_total_sim_ms_for_test(123_456);

        assert_eq!(sim_a.state_hash(), sim_b.state_hash());
    }
}

#[cfg(all(test, tests_sim_world_t_z))]
mod rocking_hash_tests {
    use super::Simulation;
    use crate::map::entities::EntityCategory;
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::rocking::RockingState;
    use crate::util::fixed_math::SimFixed;

    fn make_sim_with_one_vehicle() -> Simulation {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let type_id = sim.interner.intern("HTNK");
        let id = sim.next_stable_object_id();
        sim.set_next_stable_object_id_for_test(sim.next_stable_object_id() + 1);
        let e = GameEntity::new_at_frame_zero_for_test(
            id,
            10,
            10,
            0,
            0,
            owner,
            Health::new(400),
            type_id,
            EntityCategory::Unit,
            0,
            5,
            true,
        );
        sim.substrate.entities.insert(e);
        sim
    }

    #[test]
    fn rocking_none_and_angle_contribute_to_hash() {
        {
            let a = make_sim_with_one_vehicle();
            let b = make_sim_with_one_vehicle();
            assert_eq!(a.state_hash(), b.state_hash());

            // Mutate only the rocking state of one — hashes must diverge.
            let mut a = a;
            let id = a.substrate.entities.values().next().unwrap().stable_id();
            a.substrate.entities.get_mut(id).unwrap().rocking = Some({
                let mut value = RockingState::default();
                value.set_angle_sideways(SimFixed::lit("0.1"));
                value
            });
            assert_ne!(a.state_hash(), b.state_hash());
        }
        {
            let mut a = make_sim_with_one_vehicle();
            let b = make_sim_with_one_vehicle();
            let id = a.substrate.entities.values().next().unwrap().stable_id();
            a.substrate.entities.get_mut(id).unwrap().rocking = Some(RockingState::default());
            // a has Some(default), b has None — hashes must diverge.
            assert_ne!(a.state_hash(), b.state_hash());
        }
    }
}

#[cfg(all(test, tests_sim_world_t_z))]
mod c4_hash_tests {
    use super::Simulation;
    use crate::map::entities::EntityCategory;
    use crate::sim::c4_plant::PendingC4Detonation;
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;

    #[test]
    fn c4_state_changes_hash() {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let type_id = sim.interner.intern("GHOST");
        let id = sim.next_stable_object_id();
        sim.set_next_stable_object_id_for_test(sim.next_stable_object_id() + 1);
        let e = GameEntity::new_at_frame_zero_for_test(
            id,
            10,
            10,
            0,
            0,
            owner,
            Health::new(125),
            type_id,
            EntityCategory::Infantry,
            0,
            5,
            false,
        );
        sim.substrate.entities.insert(e);
        let h_initial = sim.state_hash();

        // Mutate pending_c4_detonation — hash must change.
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .building
            .pending_c4_detonation
            .set_for_test(Some(PendingC4Detonation::new(
                crate::sim::timer::CdTimer::started(100, 30),
                Some(7),
            )));
        let h_with_pending = sim.state_hash();
        assert_ne!(
            h_initial, h_with_pending,
            "pending_c4_detonation must affect state hash"
        );
    }
}

#[cfg(all(test, tests_sim_world_t_z))]
mod passenger_cargo_hash_tests {
    use super::Simulation;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::passenger::{PassengerCargo, PassengerRole};

    fn sim_with_sizes(first_size: f64, second_size: f64) -> Simulation {
        let mut sim = Simulation::new();
        let mut carrier = GameEntity::test_default(1, "BFRT", "Allied", 5, 5);
        let mut cargo = PassengerCargo::new(5, 0.0);
        cargo.board_forced_chain(&[(10, first_size)]);
        cargo.board_forced_chain(&[(11, second_size)]);
        carrier.set_passenger_role_for_test(PassengerRole::transport(cargo));
        sim.substrate.entities.insert(carrier);
        sim
    }

    #[test]
    fn passenger_cargo_size_mapping_and_owner_relation_change_hash() {
        {
            let a = sim_with_sizes(1.0, 3.0);
            let b = sim_with_sizes(3.0, 1.0);

            assert_ne!(a.state_hash(), b.state_hash());
        }
        {
            let mut sim = sim_with_sizes(1.0, 3.0);
            let outside = sim.state_hash();
            sim.substrate
                .entities
                .get_mut(1)
                .unwrap()
                .passenger_role_mut_for_test()
                .set_inside_transport(20, false);
            let inside = sim.state_hash();
            assert_ne!(inside, outside);
            sim.substrate
                .entities
                .get_mut(1)
                .unwrap()
                .passenger_role_mut_for_test()
                .set_inside_transport(21, false);
            assert_ne!(sim.state_hash(), inside);
            sim.substrate
                .entities
                .get_mut(1)
                .unwrap()
                .passenger_role_mut_for_test()
                .clear_carrier_relation();
            assert_eq!(sim.state_hash(), outside);
            assert_eq!(
                sim.substrate
                    .entities
                    .get(1)
                    .unwrap()
                    .passenger_role()
                    .cargo()
                    .unwrap()
                    .passengers(),
                [11, 10]
            );
        }
    }
}

#[cfg(all(test, tests_sim_world_t_z))]
mod bridge161_hash_projection_tests {
    use super::*;
    use crate::map::entities::EntityCategory;
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::combat::Health;
    use crate::sim::components::DriveCoord;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::locomotion::piggyback::LocomotorRuntimePayload;
    use crate::sim::movement::locomotor::LocomotorState;

    fn supplied_payload_world(kind: LocomotorKind, stashed: bool) -> Simulation {
        let mut sim = Simulation::new();
        let owner = sim.intern("Americans");
        let type_ref = sim.intern("HASHFOOT");
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            1,
            5,
            5,
            0,
            0,
            owner,
            Health::new(100),
            type_ref,
            EntityCategory::Infantry,
            0,
            5,
            true,
        );
        let mut locomotor = LocomotorState::for_test_kind(kind);
        if let Some(fly) = locomotor.fly_runtime_mut() {
            let tenth = crate::util::native_x87::NativeF64Bits::from_bits(0x3fb9_9999_9999_999a);
            fly.set_target_speed_bits_for_test(tenth);
            fly.set_current_speed_bits_for_test(tenth);
        }
        if stashed {
            assert!(locomotor.begin_piggyback(LocomotorKind::Teleport, 0));
        }
        entity.locomotor = Some(locomotor);
        sim.substrate.entities.insert(entity);
        sim
    }

    fn mutate(payload: &mut LocomotorRuntimePayload, field: usize) {
        let coord = DriveCoord {
            x: 1664,
            y: 1408,
            z: 414,
        };
        match payload {
            LocomotorRuntimePayload::Walk(state) => match field {
                0 => state.set_head_for_test(Some(coord)),
                1 => state.set_destination_for_test(Some(coord)),
                2 => state.set_moving_for_test(true),
                3 => state.set_animation_moving_for_test(true),
                _ => unreachable!(),
            },
            LocomotorRuntimePayload::Hover(state) => state.set_head(Some(coord)),
            LocomotorRuntimePayload::Fly(state) => {
                // Adjacent binary64 values share the same old Q16 projection.
                let next = crate::util::native_x87::NativeF64Bits::from_bits(0x3fb9_9999_9999_999b);
                match field {
                    0 => state.set_target_speed_bits_for_test(next),
                    1 => state.set_current_speed_bits_for_test(next),
                    _ => unreachable!(),
                }
            }
            LocomotorRuntimePayload::Jumpjet(state) => {
                *state = match field {
                    0 => state.clone().with_destination_for_test(coord),
                    1 => state.clone().with_moving_for_test(true),
                    2 => state.clone().with_phase_for_test(2),
                    3 => {
                        let mut flight = state.flight();
                        flight.set_target_height_for_test(7);
                        state.clone().with_flight_for_test(flight)
                    }
                    4 => {
                        let mut flight = state.flight();
                        flight.set_current_speed_bits_for_test(1.0f64.to_bits());
                        state.clone().with_flight_for_test(flight)
                    }
                    5 => {
                        let mut params = state.params();
                        params.set_speed_for_test(99);
                        state.clone().with_params_for_test(params)
                    }
                    6 => state.clone().with_landing_latched_for_test(true),
                    _ => unreachable!(),
                };
            }
            _ => unreachable!("supplied bridge payload only"),
        }
    }

    #[test]
    fn bridge161_hashes_each_active_and_stashed_payload_field() {
        // This checks Rust hash composition over supplied retained state, not
        // a native checksum or a claim that these fixtures execute movement.
        for (kind, fields) in [
            (LocomotorKind::Walk, 4),
            (LocomotorKind::Hover, 1),
            (LocomotorKind::Fly, 2),
            (LocomotorKind::Jumpjet, 7),
        ] {
            for stashed in [false, true] {
                for field in 0..fields {
                    let before = supplied_payload_world(kind, stashed);
                    let mut after = supplied_payload_world(kind, stashed);
                    let loco = after
                        .substrate
                        .entities
                        .get_mut(1)
                        .unwrap()
                        .locomotor
                        .as_mut()
                        .unwrap();
                    let object = if stashed {
                        loco.piggyback_mut().unwrap().suspended_mut_for_test()
                    } else {
                        loco
                    };
                    mutate(object.runtime_payload_mut(), field);
                    assert_ne!(
                        before.state_hash(),
                        after.state_hash(),
                        "{kind:?} stashed={stashed} field={field}"
                    );
                }
            }
        }
    }
}

#[cfg(all(test, tests_sim_world_t_z))]
mod prism_support_hash_tests {}

#[cfg(all(test, tests_sim_world_t_z))]
mod dynamic_terrain_hash_tests {
    use crate::map::resolved_terrain::{ResolvedTerrainGrid, test_flat_cell};
    use crate::sim::world::Simulation;

    /// Each process builds its TMP variant table from its own first map's
    /// draws, so two clients can draw a replaced cell with different
    /// variants; the state hash must not see it, but does see its level.
    #[test]
    fn a_replaced_cells_variant_does_not_reach_the_state_hash() {
        let hash = |edit: fn(&mut crate::map::resolved_terrain::ResolvedTerrainCell)| {
            let mut sim = Simulation::new();
            sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(
                4,
                4,
                (0..4)
                    .flat_map(|y| (0..4).map(move |x| test_flat_cell(x, y)))
                    .collect(),
            ));
            let mut cell = test_flat_cell(1, 2);
            edit(&mut cell);
            sim.dynamic_terrain_cells_mut_for_test().record(&cell);
            sim.state_hash()
        };
        let drawn = hash(|_| {});
        assert_eq!(
            hash(|cell| {
                cell.variant = 3;
                cell.render_offset_x = 30;
                cell.radar_left = [1, 2, 3];
            }),
            drawn
        );
        assert_ne!(hash(|cell| cell.level = 4), drawn);
    }
}
