//! Reveal placement tests: the Z a revealed object takes (floor, ramp and
//! drop heights), Drive/Ship slope snapping, playfield membership at
//! unlimbo and campaign discovery on cell put.

use super::*;

/// CellPUT47E96E..47E9DC calls Player discovery for an unshrouded campaign
/// cell; IsFogged5865E0 always returns false in active YR. The original
/// foreign-owner entry/PUT contrast retains41C and changes41B only in the
/// admitted cases. This protects the shared gate used by Foot and Building.
#[test]
fn native_trigger_reveal_cell_put_discovers_unshrouded_campaign_resident() {
    use crate::sim::vision::{FogState, OwnerVisibility};
    use crate::sim::world::TechnoDiscoveryHistory;

    for (game_mode_nonzero, revealed, discovered) in [
        (false, false, false),
        (false, true, true),
        (true, false, true),
    ] {
        let mut sim = Simulation::with_seed(31);
        let player = sim.interner.intern("Player");
        let owner = sim.interner.intern("Other");
        sim.session.set_current_house_for_test(Some(player));
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(game_mode_nonzero),
        );
        for house in [player, owner] {
            sim.houses
                .register(HouseState::new(house, 0, None, true, 0, 10));
        }
        sim.install_resolved_terrain_for_new_map(crate::map::resolved_terrain::test_grid(
            24,
            24,
            crate::map::resolved_terrain::test_clear_cell,
        ));
        *sim.fog = FogState::new(24, 24);
        let mut plane = OwnerVisibility::new(24, 24);
        if revealed {
            plane.mark_visible(10, 10);
        }
        sim.fog.by_owner_mut_for_test().insert(player, plane);
        let typ = sim.interner.intern("E1");
        let mut actor = GameEntity::new_at_frame_for_test(
            1,
            10,
            10,
            0,
            0,
            owner,
            Health::new(100),
            typ,
            EntityCategory::Infantry,
            0,
            0,
            false,
            0,
        );
        actor.seldom.discovery = TechnoDiscoveryHistory::for_test(false, false, true);
        let id = sim.substrate.entities.insert(actor);
        let scenario = sim.scenario_rng.native_state_hex();
        let main = sim.main_rng.native_state_hex();
        let mapgen = sim.mapgen_rng.native_state_hex();
        sim.discover_by_player(id, Some((10, 10)), super::UninitContext::default());
        let history = sim.substrate.entities.get(id).unwrap().seldom.discovery;
        assert_eq!(
            history.discovered_by_current_house(),
            discovered,
            "mode_nonzero={game_mode_nonzero}, revealed={revealed}"
        );
        assert!(!history.owned_by_current_house() && history.discovered_by_other_house());
        assert_eq!(sim.houses[&owner].discovered_by_current_house(), discovered);
        assert_eq!(sim.scenario_rng.native_state_hex(), scenario);
        assert_eq!(sim.main_rng.native_state_hex(), main);
        assert_eq!(sim.mapgen_rng.native_state_hex(), mapgen);
    }
}

fn drive_ship_slope_rules() -> crate::rules::ruleset::RuleSet {
    let ini = crate::rules::ini_parser::IniFile::from_str(
        "[VehicleTypes]\n0=DRIVE\n1=SHIP\n2=TTRAIN\n\
         [DRIVE]\nStrength=100\nSpeed=6\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
         [SHIP]\nStrength=100\nSpeed=6\nLocomotor={2BEA74E1-7CCA-11D3-BE14-00104B62A16C}\n\
         [TTRAIN]\nStrength=100\nSpeed=6\nIsTrain=yes\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n",
    );
    crate::rules::ruleset::RuleSet::from_ini(&ini).expect("Drive/Ship slope rules")
}

fn zero_speed_drive_ship_slope_rules() -> crate::rules::ruleset::RuleSet {
    let ini = crate::rules::ini_parser::IniFile::from_str(
        "[VehicleTypes]\n0=ZUNITD\n1=ZMAPD\n2=ZLIMBOS\n3=ZWALK\n\
         [InfantryTypes]\n0=ZINFD\n\
         [AircraftTypes]\n0=ZAIRS\n\
         [BuildingTypes]\n0=ZBUILDD\n\
         [ZUNITD]\nStrength=100\nSpeed=0\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
         [ZMAPD]\nStrength=100\nSpeed=0\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
         [ZLIMBOS]\nStrength=100\nSpeed=0\nLocomotor={2BEA74E1-7CCA-11D3-BE14-00104B62A16C}\n\
         [ZWALK]\nStrength=100\nSpeed=0\nLocomotor={4A582744-9839-11D1-B709-00A024DDAFD1}\n\
         [ZINFD]\nStrength=100\nSpeed=0\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
         [ZAIRS]\nStrength=100\nSpeed=0\nLocomotor={2BEA74E1-7CCA-11D3-BE14-00104B62A16C}\n\
         [ZBUILDD]\nStrength=100\nSpeed=0\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n",
    );
    crate::rules::ruleset::RuleSet::from_ini(&ini).expect("zero-speed Drive/Ship rules")
}

#[test]
fn grounded_ramp_reveal_commits_native_height_before_occupation() {
    // Ground=52 and deck=468 are original-code results in
    // tools/ramp_height_vectors.json: ramp_1_sub_128_128 and
    // signed_level_0_bridge_1. Elevated input is a coarse API preservation case.
    for (category, kind) in [
        (EntityCategory::Unit, LocomotorKind::Drive),
        (EntityCategory::Unit, LocomotorKind::Ship),
        (EntityCategory::Infantry, LocomotorKind::Walk),
    ] {
        for (on_bridge, requested_level, expected_z, deck) in [
            (false, 0, 52, false),
            (true, 4, 468, true),
            (false, 4, 416, false),
            (true, 7, 728, true),
        ] {
            let mut sim = Simulation::with_seed(71);
            install_common_raw_terrain(&mut sim, 8, 8, 0, Some((2, 2)));
            sim.resolved_terrain
                .as_mut()
                .unwrap()
                .cell_mut_for_test(2, 2)
                .unwrap()
                .slope_type = 1;
            insert_entity(&mut sim, 1, category);
            let entity = sim.substrate.entities.get_mut(1).unwrap();
            entity.locomotor = Some(LocomotorState::for_test_kind(kind));
            entity.set_on_bridge(on_bridge);
            let before_rng = sim.scenario_rng.logical_state();
            assert!(matches!(
                sim.try_reveal_entity(1, common_raw_request(2, 2, requested_level, 128, 128)),
                RevealOutcome::Revealed { .. }
            ));
            assert_eq!(
                sim.substrate
                    .entities
                    .get(1)
                    .unwrap()
                    .position
                    .exact_z_leptons(),
                Some(expected_z),
                "{kind:?} bridge={on_bridge} level={requested_level}"
            );
            // An infantryman's bits come from InfantryClass::Unlimbo, which
            // skips a man placed more than a deck above the ground
            // (`0x0051E0F6..0x0051E10E`), not from his Mark.
            let marked = deck && (category == EntityCategory::Unit || expected_z <= 52 + 416);
            assert_eq!(
                sim.substrate.raw_cell_occupation.deck_bits(2, 2) != 0,
                marked,
                "Mark(PUT) must read the committed ramp/deck coordinate"
            );
            assert_eq!(sim.scenario_rng.logical_state(), before_rng);
        }
    }
}

/// A building's Unlimbo coordinate passes through BuildingType virtual +0x6C
/// (`0x00464A70`): the floor at its XY, whatever the input Z, and never a
/// deck. Ground=52 is the original-code result ramp_1_sub_128_128 in
/// tools/ramp_height_vectors.json.
#[test]
fn a_revealed_building_takes_the_floor_at_its_location_as_its_z() {
    for (on_bridge, requested_level) in [(false, 0), (false, 4), (true, 4)] {
        let mut sim = Simulation::with_seed(71);
        install_common_raw_terrain(&mut sim, 8, 8, 0, Some((2, 2)));
        sim.resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut_for_test(2, 2)
            .unwrap()
            .slope_type = 1;
        insert_entity(&mut sim, 1, EntityCategory::Structure);
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .set_on_bridge(on_bridge);
        assert!(matches!(
            sim.try_reveal_entity(1, common_raw_request(2, 2, requested_level, 128, 128)),
            RevealOutcome::Revealed { .. }
        ));
        let building = sim.substrate.entities.get(1).unwrap();
        assert_eq!(
            building.position.exact_z_leptons(),
            Some(52),
            "bridge={on_bridge} level={requested_level}"
        );
        assert_eq!(
            crate::sim::movement::ground_pose::position_world_coord(&building.position).z,
            52
        );
    }
}

/// UnitType's Unlimbo coordinate adjustment (virtual +0x6C, `0x00747EB0`)
/// clamps the input Z to the floor whatever the locomotor, so a Hover, Fly,
/// Jumpjet, Rocket or Teleport unit starts on the floor as a Drive one does,
/// at GetHeight 0.
/// Ground=52 is the original-code result ramp_1_sub_128_128 in
/// tools/ramp_height_vectors.json.
#[test]
fn every_unit_locomotor_takes_the_floor_clamp_at_unlimbo() {
    for kind in [
        LocomotorKind::Hover,
        LocomotorKind::Fly,
        LocomotorKind::Jumpjet,
        LocomotorKind::Rocket,
        LocomotorKind::Teleport,
    ] {
        let mut sim = Simulation::new();
        install_common_raw_terrain(&mut sim, 8, 8, 0, None);
        sim.resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut_for_test(2, 2)
            .unwrap()
            .slope_type = 1;
        insert_entity(&mut sim, 1, EntityCategory::Unit);
        sim.substrate.entities.get_mut(1).unwrap().locomotor =
            Some(LocomotorState::for_test_kind(kind));
        assert!(matches!(
            sim.try_reveal_entity(1, common_raw_request(2, 2, 0, 128, 128)),
            RevealOutcome::Revealed { .. }
        ));
        let unit = sim.substrate.entities.get(1).unwrap();
        assert_eq!(unit.position.exact_z_leptons(), Some(52), "{kind:?}");
        assert_eq!(
            crate::sim::movement::air_movement::current_fly_height(
                unit,
                sim.resolved_terrain.as_ref()
            ),
            0,
            "{kind:?}"
        );
    }
}

/// A reveal without terrain keeps no exact Z, and a falling object keeps
/// the Z its drop gave it.
#[test]
fn grounded_reveal_keeps_headless_inputs_and_the_drop_z() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Infantry);
    sim.substrate.entities.get_mut(1).unwrap().locomotor =
        Some(LocomotorState::for_test_kind(LocomotorKind::Walk));
    assert!(matches!(
        sim.try_reveal_entity(1, common_raw_request(2, 2, 3, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .position
            .exact_z_leptons(),
        None
    );

    // Actual paradrop ordering: attach while limbo, then Reveal. Paradrop's
    // Unlimbo coordinate is the drop coordinate (0x005F5A50), so the ground
    // adapter keeps the drop's Z rather than the passenger's old one.
    install_common_raw_terrain(&mut sim, 8, 8, 0, None);
    insert_entity(&mut sim, 2, EntityCategory::Infantry);
    let entity = sim.substrate.entities.get_mut(2).unwrap();
    entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Walk));
    entity.position.set_exact_z_leptons_for_test(Some(52));
    assert!(
        crate::sim::movement::parachute_descent::begin_parachute_descent(
            &mut sim.substrate.entities,
            2,
            1200
        )
    );
    assert!(matches!(
        sim.try_reveal_entity(2, common_raw_request(2, 2, 0, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));
    let entity = sim.substrate.entities.get(2).unwrap();
    assert_eq!(entity.position.exact_z_leptons(), Some(1200));
    assert!(entity.is_falling_down());
}

#[test]
fn grounded_ramp_position_survives_snapshot_and_idle_continuation() {
    let mut sim = Simulation::with_seed(71);
    assert_eq!(sim.substrate.allocate_stable_id(), 1);
    install_common_raw_terrain(&mut sim, 8, 8, 0, None);
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut_for_test(2, 2)
        .unwrap()
        .slope_type = 1;
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    sim.substrate.entities.get_mut(1).unwrap().locomotor =
        Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    assert!(matches!(
        sim.try_reveal_entity(1, common_raw_request(2, 2, 0, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));
    // Native snapshot load resets this stream. Canonicalize the original too
    // so whole-world comparison measures continuation, not that load policy.
    *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let bytes = GameSnapshot::save(&sim, 0, 0, "ramp-height", 0);
    let mut restored = GameSnapshot::load(&bytes).expect("ramp snapshot").sim;
    restored
        .restore_after_snapshot_load()
        .expect("ramp restore");
    install_common_raw_terrain(&mut restored, 8, 8, 0, None);
    restored
        .resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut_for_test(2, 2)
        .unwrap()
        .slope_type = 1;
    assert_eq!(
        restored
            .substrate
            .entities
            .get(1)
            .unwrap()
            .position
            .exact_z_leptons(),
        Some(52)
    );
    for _ in 0..3 {
        sim.advance_tick(&[], None, None, None, 66);
        restored.advance_tick(&[], None, None, None, 66);
        assert_eq!(
            restored
                .substrate
                .entities
                .get(1)
                .unwrap()
                .position
                .exact_z_leptons(),
            Some(52)
        );
        assert_eq!(restored.state_hash(), sim.state_hash());
    }
}

#[test]
fn drive_ship_slope_production_spawn_unlimbo_snaps_without_manual_rocking_state() {
    let rules = drive_ship_slope_rules();
    for (type_id, cell, slope, native_z) in [
        ("DRIVE", (2, 2), 5, 0),
        ("SHIP", (4, 2), 9, 104),
        ("TTRAIN", (6, 2), 12, 104),
    ] {
        let mut sim = Simulation::with_seed(0x510fe);
        sim.session.set_binary_frame_for_test(37);
        *sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
            base: 0,
            off_fc: -100,
            off_100: -100,
            off_104: 200,
            off_108: 200,
        });
        install_common_raw_terrain(&mut sim, 10, 5, 0, None);
        let terrain_cell = sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut_for_test(cell.0, cell.1)
            .unwrap();
        terrain_cell.slope_type = slope;
        terrain_cell.speed_costs = SpeedCostProfile {
            foot: Some(crate::util::native_x87::NativeF32Bits::ONE),
            track: Some(crate::util::native_x87::NativeF32Bits::ONE),
            wheel: Some(crate::util::native_x87::NativeF32Bits::ONE),
            float: Some(crate::util::native_x87::NativeF32Bits::ONE),
            amphibious: Some(crate::util::native_x87::NativeF32Bits::ONE),
            float_beach: Some(crate::util::native_x87::NativeF32Bits::ONE),
            hover: Some(crate::util::native_x87::NativeF32Bits::ONE),
            winged: Some(crate::util::native_x87::NativeF32Bits::ONE),
        };
        terrain_cell.base_speed_costs = terrain_cell.speed_costs;
        let mut expected_rng = sim.scenario_rng.clone();
        let _constructor_word = expected_rng.next_u32();

        let stable_id = sim
            .spawn_object(type_id, "Americans", cell.0, cell.1, 0, &rules)
            .expect("production spawn/unlimbo");
        let entity = sim.substrate.entities.get(stable_id).unwrap();
        // Original-code ramp_{5,9,12}_sub_128_128 fixtures; this reaches the
        // production constructor and Reveal, with no preloaded exact pose.
        assert_eq!(entity.position.exact_z_leptons(), Some(native_z));
        assert!(
            entity.rocking.is_none(),
            "slope state is not manually injected"
        );
        assert_eq!(
            crate::sim::movement::slope_transition::state_for_entity(entity)
                .expect("active Drive/Ship state")
                .hash_fields(),
            (slope, slope, 37, 0),
            "successful Foot unlimbo snaps both slope bytes at the current frame"
        );
        assert_eq!(
            sim.scenario_rng.logical_state(),
            expected_rng.logical_state(),
            "the Techno constructor consumes its one proven word and slope Unlimbo adds no draw"
        );
    }
}

#[test]
fn zero_speed_foot_payloads_survive_all_world_spawn_paths() {
    let rules = zero_speed_drive_ship_slope_rules();
    let mut sim = Simulation::with_seed(0x510fe);
    sim.session.set_binary_frame_for_test(71);
    *sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
        base: 0,
        off_fc: -100,
        off_100: -100,
        off_104: 200,
        off_108: 200,
    });
    install_common_raw_terrain(&mut sim, 12, 8, 0, None);
    for (cell, slope) in [((2, 2), 5), ((4, 2), 8), ((6, 2), 11), ((8, 2), 14)] {
        let terrain_cell = sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut_for_test(cell.0, cell.1)
            .unwrap();
        terrain_cell.slope_type = slope;
        terrain_cell.speed_costs = SpeedCostProfile {
            foot: Some(crate::util::native_x87::NativeF32Bits::ONE),
            track: Some(crate::util::native_x87::NativeF32Bits::ONE),
            wheel: Some(crate::util::native_x87::NativeF32Bits::ONE),
            float: Some(crate::util::native_x87::NativeF32Bits::ONE),
            amphibious: Some(crate::util::native_x87::NativeF32Bits::ONE),
            float_beach: Some(crate::util::native_x87::NativeF32Bits::ONE),
            hover: Some(crate::util::native_x87::NativeF32Bits::ONE),
            winged: Some(crate::util::native_x87::NativeF32Bits::ONE),
        };
        terrain_cell.base_speed_costs = terrain_cell.speed_costs;
    }

    for (type_id, cell, expected_kind, slope) in [
        ("ZUNITD", (2, 2), LocomotorKind::Drive, 5),
        ("ZINFD", (4, 2), LocomotorKind::Drive, 8),
        ("ZAIRS", (6, 2), LocomotorKind::Ship, 11),
    ] {
        let stable_id = sim
            .spawn_object(type_id, "Americans", cell.0, cell.1, 0, &rules)
            .expect("zero-speed Foot production spawn/reveal");
        let entity = sim.substrate.entities.get(stable_id).unwrap();
        assert_eq!(
            entity.locomotor.as_ref().unwrap().active_kind(),
            expected_kind
        );
        assert_eq!(
            crate::sim::movement::slope_transition::state_for_entity(entity)
                .expect("constructor-owned Drive/Ship payload")
                .hash_fields(),
            (slope, slope, 71, 0),
            "successful reveal snaps the payload without test injection"
        );
    }

    let placement = MapEntity {
        owner: "Americans".to_string(),
        type_id: "ZMAPD".to_string(),
        trigger_tag: None,
        health: 256,
        cell_x: 8,
        cell_y: 2,
        facing: 0,
        category: EntityCategory::Unit,
        sub_cell: 0,
        veterancy: Some(0),
        high: false,
        mission: None,
        recruitable_a: true,
        recruitable_b: true,
        group: -1,
        follows: None,
        structure_columns: Default::default(),
        structure_ai_sellable: false,
        structure_ai_repairable: false,
        structure_online: true,
    };
    assert_eq!(sim.spawn_from_map(&[placement], Some(&rules)), 1);
    let map_entity = sim
        .substrate
        .entities
        .values()
        .find(|entity| sim.interner.resolve(entity.type_ref()) == "ZMAPD")
        .unwrap();
    assert_eq!(
        crate::sim::movement::slope_transition::state_for_entity(map_entity)
            .unwrap()
            .hash_fields(),
        (14, 14, 71, 0),
        "scenario world spawn also constructs and reveals the zero-speed Drive payload"
    );

    let limbo_ship = sim
        .spawn_object_limbo_at_height("ZLIMBOS", "Americans", 10, 2, 0, 0, &rules)
        .expect("zero-speed limbo Ship");
    assert_eq!(
        crate::sim::movement::slope_transition::state_for_entity(
            sim.substrate.entities.get(limbo_ship).unwrap()
        )
        .unwrap()
        .hash_fields(),
        (0, 0, 71, 0),
        "limbo construction owns fresh class state without an Unlimbo snap"
    );

    let building = sim
        .spawn_object_limbo_at_height("ZBUILDD", "Americans", 10, 3, 0, 0, &rules)
        .unwrap();
    assert!(
        sim.substrate
            .entities
            .get(building)
            .unwrap()
            .locomotor
            .is_none(),
        "a Structure owns no Foot instance"
    );
    let walker = sim
        .spawn_object_limbo_at_height("ZWALK", "Americans", 10, 3, 0, 0, &rules)
        .unwrap();
    assert_eq!(
        sim.substrate
            .entities
            .get(walker)
            .unwrap()
            .locomotor
            .as_ref()
            .expect("the Unit constructor (0x7354CE) builds its instance at Speed 0")
            .active_kind(),
        LocomotorKind::Walk,
        "a zero-speed Walk Unit keeps its constructed locomotor"
    );
}

#[test]
fn drive_ship_slope_failed_reveal_does_not_snap_or_consume_rng() {
    let mut sim = Simulation::with_seed(0x510fe);
    sim.session.set_binary_frame_for_test(40);
    install_common_raw_terrain(&mut sim, 8, 8, 0, None);
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut_for_test(3, 4)
        .unwrap()
        .slope_type = 8;
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    sim.substrate.entities.get_mut(1).unwrap().locomotor = Some(
        LocomotorState::for_test_kind_at_frame(LocomotorKind::Drive, 11),
    );
    let rng_before = sim.scenario_rng.logical_state();

    assert_eq!(
        sim.try_reveal_entity(1, request(3, 4, PlacementEvidence::MarkFailed)),
        RevealOutcome::Failed(RevealFailure::MarkFailed)
    );
    assert_eq!(
        crate::sim::movement::slope_transition::state_for_entity(
            sim.substrate.entities.get(1).unwrap()
        )
        .unwrap()
        .hash_fields(),
        (0, 0, 11, 0)
    );
    assert_eq!(sim.scenario_rng.logical_state(), rng_before);
}

#[test]
fn techno_playfield_ctor_unlimbo_and_movement_hysteresis() {
    use crate::map::playfield::PlayfieldBounds;

    let mut sim = Simulation::new();
    install_common_raw_terrain(&mut sim, 32, 32, 0, None);
    let bounds = PlayfieldBounds::from_normalized_local_size(32, 2, 2, 24, 20);
    *sim.playfield_bounds = Some(bounds);
    let inside = (0u16..32)
        .flat_map(|ry| (0u16..32).map(move |rx| (rx, ry)))
        .find(|&(rx, ry)| bounds.contains_height_aware_packed(rx.into(), ry.into(), 0, 0))
        .expect("mode-one inside cell");
    let outside = (0u16..32)
        .flat_map(|ry| (0u16..32).map(move |rx| (rx, ry)))
        .find(|&(rx, ry)| !bounds.contains_height_aware_packed(rx.into(), ry.into(), 0, 0))
        .expect("mode-one outside cell");

    insert_entity(&mut sim, 1, EntityCategory::Unit);
    assert!(!sim.substrate.entities.get(1).unwrap().in_playfield());
    let mut refused = common_raw_request(inside.0, inside.1, 0, 128, 128);
    refused.placement = PlacementEvidence::MarkFailed;
    assert_eq!(
        sim.try_reveal_entity(1, refused),
        RevealOutcome::Failed(RevealFailure::MarkFailed)
    );
    assert!(
        !sim.substrate.entities.get(1).unwrap().in_playfield(),
        "Techno6F6CB8 returns before the +3D5 writer when Object Mark fails"
    );
    assert!(matches!(
        sim.try_reveal_entity(1, common_raw_request(inside.0, inside.1, 0, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));
    assert!(
        sim.substrate.entities.get(1).unwrap().in_playfield(),
        "TechnoClass::Unlimbo @ 0x006F6CFE establishes exact mode-one membership"
    );

    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .set_in_playfield(false);
    sim.promote_entity_playfield_membership_after_move(1);
    assert!(sim.substrate.entities.get(1).unwrap().in_playfield());
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .position
        .set_rx_for_test(outside.0);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .position
        .set_ry_for_test(outside.1);
    sim.promote_entity_playfield_membership_after_move(1);
    assert!(
        sim.substrate.entities.get(1).unwrap().in_playfield(),
        "ordinary Foot movement @ 0x006F511A promotes but never demotes"
    );
}
