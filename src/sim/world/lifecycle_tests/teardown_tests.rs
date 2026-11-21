//! Destruction teardown tests: constructor-complete Foot destructors, the
//! deferred delete queue, receiver fatal limbo and the smudge clearing a
//! dying structure performs.

use super::*;

#[test]
fn constructor_complete_foot_destructors_match_native_team_gate_and_rng() {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::team_ai_ini::TeamAiIniRegistry;
    use crate::sim::rng::SimRng;

    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art() else {
        return;
    };
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/team_creation.json",
    ))
    .unwrap();
    let rows: Vec<_> = corpus
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["case_kind"] == "scalar_cleanup")
        .collect();
    assert_eq!(rows.len(), 12);
    for row in rows {
        let input = &row["input"];
        let typ = input["type"].as_str().unwrap();
        let mut sim = Simulation::new();
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.install_resolved_terrain_for_new_map(crate::map::resolved_terrain::test_grid(
            128,
            128,
            |x, y| {
                let mut cell = crate::map::resolved_terrain::test_flat_cell(x, y);
                cell.level = 4;
                cell
            },
        ));
        sim.session.set_binary_frame_for_test(1);
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        if input["scenario_started_byte"].as_i64().unwrap() != 0 {
            sim.session.publish_started();
        }
        let owner = sim.interner.intern("Americans");
        sim.houses
            .register(HouseState::new(owner, 0, Some(owner), true, 0, 10));
        let definitions = IniFile::from_str(&format!(
            "[TeamTypes]\n0=TT\n[TT]\nHouse=Americans\nScript=S\nTaskForce=F\nWaypoint=E\nMax=-1\n[ScriptTypes]\n0=S\n[S]\n0=3,4\n[TaskForces]\n0=F\n[F]\n0=1,{typ}\n"
        ));
        let registry = TeamAiIniRegistry::from_sources(
            &definitions,
            &IniFile::from_str(""),
            RulePolicy::stock_multiplayer(),
        );
        sim.install_team_ai_registry(&registry, &rules);
        let team_type = sim.interner.get("TT").unwrap();
        let team = sim
            .with_object_placement_scope(|sim| sim.create_team(team_type, Some(owner), &rules))
            .unwrap();
        let member = sim
            .with_object_placement_scope(|sim| {
                sim.construct_object_limbo_at_height(
                    typ,
                    "Americans",
                    0,
                    0,
                    0,
                    0,
                    &rules,
                    &mut crate::sim::world::FrameEffects::default(),
                )
            })
            .unwrap();
        assert!(sim.with_object_placement_scope(|sim| {
            sim.team_add_member(
                team,
                member,
                false,
                &rules,
                &mut super::FrameEffects::default(),
            )
        }));
        let actor = sim.entities().get(member).unwrap();
        assert!(actor.lifecycle.object_alive() && actor.lifecycle.in_limbo());
        assert_eq!(row["prior"]["alive"], 1);
        assert_eq!(row["prior"]["limbo"], 1);
        assert_eq!(row["prior"]["type_present"], true);
        let counts = |field: &str| {
            row[field]["taskforce_counts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| i32::try_from(value.as_i64().unwrap()).unwrap())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            sim.team_script_vm
                .team(team)
                .unwrap()
                .taskforce_counts_for_test()
                .as_slice(),
            counts("prior"),
            "{input}"
        );
        // The native prior is observed at genuine action7's first scalar
        // entry, not an invented dead/queue-ready object. This comparison
        // starts its RNG there; the action7 test separately covers constructor
        // draws and native identities. No process-Started inference is made.
        sim.scenario_rng =
            SimRng::from_native_state_hex_for_test(row["rng_before"]["scenario"].as_str().unwrap());
        sim.main_rng =
            SimRng::from_native_state_hex_for_test(row["rng_before"]["main"].as_str().unwrap());
        sim.mapgen_rng =
            SimRng::from_native_state_hex_for_test(row["rng_before"]["mapgen"].as_str().unwrap());
        sim.finalize_and_remove_common(
            member,
            super::lifecycle::UninitContext::new(&rules),
            i32::try_from(input["game_active_byte"].as_i64().unwrap()).unwrap(),
        );
        assert!(!sim.entities().contains(member), "{input}");
        let state = sim.team_script_vm.team(team).unwrap();
        assert_eq!(
            state.taskforce_counts_for_test().as_slice(),
            counts("after_dtor"),
            "{input}"
        );
        assert_eq!(
            state.members().next().is_some(),
            row["after_dtor"]["head_present"].as_bool().unwrap(),
            "{input}"
        );
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            row["rng_after"]["scenario"].as_str().unwrap(),
            "{input}"
        );
        assert_eq!(
            sim.main_rng.native_state_hex(),
            row["rng_after"]["main"].as_str().unwrap(),
            "{input}"
        );
        assert_eq!(
            sim.mapgen_rng.native_state_hex(),
            row["rng_after"]["mapgen"].as_str().unwrap(),
            "{input}"
        );
        // Native inactive teardown retains +48/+4C bookkeeping although its
        // linked head is NULL. Team.members models that head/list; its length
        // is not a claim about these separate native retired-state counters.
    }
}

#[test]
fn deferred_queue_indexes_and_saved_continuation_handle_twenty_thousand_identities() {
    use super::{DeferredDeleteQueue, DeferredDeleteTicket};
    use std::hash::Hasher;

    // Native725C70 ordering is pinned by the mixed executable histories in
    // trigger_runtime_native_tests. This protects the representation at the
    // target scale: holes, duplicates and later appends cross every index
    // growth boundary, then the serialized order must resume identically.
    let mut queue = DeferredDeleteQueue::default();
    for id in 0..20_000 {
        queue.queue_object(id);
        if id % 13 == 0 {
            queue.queue_object(id);
        }
    }
    for id in (0..20_000).step_by(3) {
        queue.remove_object(id);
    }
    queue.queue_tag(31);
    queue.queue_trigger(47);
    queue.queue_overlay(20_001).unwrap();
    queue.assert_consistent_for_test();
    let bytes = save_codec::serialize(&queue).unwrap();
    let mut restored: DeferredDeleteQueue = save_codec::deserialize(&bytes).unwrap();
    restored.assert_consistent_for_test();
    assert_eq!(queue, restored);

    for id in 20_002..40_002 {
        queue.queue_object(id);
        restored.queue_object(id);
        if id % 7 == 0 {
            queue.remove_object(id - 20_002);
            restored.remove_object(id - 20_002);
        }
    }
    queue.remove(DeferredDeleteTicket::Tag(31));
    restored.remove(DeferredDeleteTicket::Tag(31));
    queue.finish_drain();
    restored.finish_drain();
    queue.assert_consistent_for_test();
    restored.assert_consistent_for_test();
    assert_eq!(queue, restored);
    assert_eq!(
        save_codec::serialize(&queue).unwrap(),
        save_codec::serialize(&restored).unwrap()
    );
    let hash = |queue: &DeferredDeleteQueue| {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        queue.hash_state(&mut hasher);
        hasher.finish()
    };
    assert_eq!(hash(&queue), hash(&restored));
}

#[test]
fn receiver_fatal_limbo_clears_elevated_ground_occupation() {
    let mut sim = Simulation::new();
    install_common_raw_terrain(&mut sim, 16, 16, 4, None);
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    assert!(matches!(
        sim.try_reveal_entity(1, common_raw_request(3, 4, 4, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0x20);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);

    commit_receiver_authority_lethal_hit(&mut sim, 1);

    assert!(sim.substrate.entities.get(1).unwrap().lifecycle.in_limbo());
    assert_eq!(
        sim.substrate.raw_cell_occupation.ground_bits(3, 4),
        0,
        "fatal Limbo must clear the elevated ground plane marked by Reveal"
    );
}

#[test]
fn receiver_fatal_limbo_clears_sparse_map_dummy_reservation() {
    let mut sim = Simulation::new();
    install_common_raw_terrain(&mut sim, 16, 16, 0, None);
    let allocated: Vec<_> = (0..16)
        .flat_map(|y| (0..16).map(move |x| (x, y)))
        .filter(|&cell| cell != (9, 9))
        .collect();
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .test_set_native_allocated_cells(&allocated);
    insert_reservation_building(&mut sim, 41, "Americans", 10, 10, &"1x1".into(), 1);
    assert!(matches!(
        sim.try_reveal_entity(41, common_raw_request(10, 10, 0, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));
    assert_ne!(sim.substrate.base_reservations.dummy_mask(), 0);

    commit_receiver_authority_lethal_hit(&mut sim, 41);

    assert!(sim.substrate.entities.get(41).unwrap().lifecycle.in_limbo());
    assert_eq!(
        sim.substrate.base_reservations.dummy_mask(),
        0,
        "fatal Limbo must resolve the same sparse-map CellClass identity as Reveal"
    );
}

#[test]
fn receiver_garrison_survivor_keeps_height_aware_playfield_membership() {
    let rules = crate::rules::ruleset::RuleSet::from_ini_with_fixed_art_for_test(
        &crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n0=OCCUPANT\n[BuildingTypes]\n0=GARRISON\n\
             [OCCUPANT]\nStrength=100\nArmor=none\n\
             [GARRISON]\nStrength=100\nArmor=concrete\nCanBeOccupied=yes\n\
             [Warheads]\n0=KILLWH\n[KILLWH]\nCellSpread=0\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        ),
        &crate::rules::ini_parser::IniFile::from_str("[GARRISON]\nFoundation=2x2\n"),
    )
    .expect("garrison survivor rules");
    let mut sim = Simulation::new();
    sim.session.set_map_width_for_test(16);
    sim.session.set_map_height_for_test(16);
    sim.playfield_bounds =
        Some(crate::sim::cell_rect::PlayfieldBounds::from_normalized_local_size(16, 2, 2, 12, 3));
    install_common_raw_terrain(&mut sim, 16, 16, 4, None);
    // SellBuilding's exit probe is the occupant's own Can_Enter_Cell, which
    // reads the cells' speed rows.
    let terrain = sim.resolved_terrain.as_mut().unwrap();
    for (rx, ry) in (0..16u16).flat_map(|ry| (0..16u16).map(move |rx| (rx, ry))) {
        let cell = terrain.cell_mut_for_test(rx, ry).unwrap();
        cell.speed_costs = crate::map::resolved_terrain::TEST_OPEN_SPEED_COSTS;
        cell.base_speed_costs = cell.speed_costs;
    }
    let building_id = sim.substrate.allocate_stable_id();
    insert_entity(&mut sim, building_id, EntityCategory::Structure);
    let passenger_id = sim.substrate.allocate_stable_id();
    insert_entity(&mut sim, passenger_id, EntityCategory::Infantry);
    let building_type = sim.interner.intern("GARRISON");
    let passenger_type = sim.interner.intern("OCCUPANT");
    {
        let passenger = sim.substrate.entities.get_mut(passenger_id).unwrap();
        passenger.set_type_ref_for_test(passenger_type);
        passenger.set_passenger_role_for_test(PassengerRole::inside_for_test(building_id, false));
    }
    {
        let building = sim.substrate.entities.get_mut(building_id).unwrap();
        building.set_type_ref_for_test(building_type);
        building
            .seldom
            .type_facts
            .set_foundation_for_test("2x2".to_string().into());
        building
            .passenger_role_mut_for_test()
            .occupants_mut()
            .add_for_test(passenger_id);
    }
    assert!(matches!(
        sim.try_reveal_entity_with_context(
            building_id,
            common_raw_request(13, 13, 4, 128, 128),
            super::UninitContext::new(&rules)
        ),
        RevealOutcome::Revealed { .. }
    ));
    assert!(
        sim.substrate
            .entities
            .get(building_id)
            .unwrap()
            .cached_spatial_threat()
            .is_some()
    );
    assert!(
        sim.substrate
            .entities
            .get(building_id)
            .unwrap()
            .in_playfield()
    );

    let warhead = sim.interner.intern("KILLWH");
    let hit = crate::sim::combat::EntityDamageEvent::direct_receiver(
        building_id,
        100,
        0,
        crate::sim::combat::RAD_NO_ATTACKER,
        None,
        warhead,
        crate::sim::combat::ReceiverCallFlags {
            ignore_defenses: true,
            arg6: false,
        },
    );
    sim.commit_noncombat_aoe_hits(
        &rules,
        None,
        &[hit],
        &mut crate::sim::world::FrameEffects::default(),
    );

    let passenger = sim.substrate.entities.get(passenger_id).unwrap();
    assert!(passenger.lifecycle.object_alive());
    assert!(!passenger.lifecycle.in_limbo());
    assert_eq!(
        (
            passenger.position.rx(),
            passenger.position.ry(),
            passenger.position.z()
        ),
        (15, 15, 4)
    );
    // SellBuilding @ 0x00458060 calls Infantry Unlimbo; Techno Unlimbo
    // @ 0x006F6CC0..0x006F6CFE writes mode-one global MapClass membership,
    // including during the ejection helper's temporary editor-mode bracket.
    assert!(
        passenger.in_playfield(),
        "survivor must use actual level-four map membership"
    );
}

#[test]
fn uninit_clears_aircraft_bridge_occupation() {
    let mut sim = Simulation::new();
    install_common_raw_terrain(&mut sim, 8, 8, 1, Some((3, 4)));
    install_fly_aircraft(&mut sim, 1, SimFixed::from_num(0));
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .set_on_bridge(true);
    assert!(matches!(
        sim.try_reveal_entity(1, common_raw_request(3, 4, 5, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0x40);

    // UnInit itself, not Aircraft ReceiveDamage: fatal Aircraft damage
    // starts a crash and does not immediately UnInit.
    sim.uninit_with_context(1, super::UninitContext::default());

    assert!(sim.substrate.entities.get(1).unwrap().lifecycle.in_limbo());
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);
}

#[test]
fn gsi_04_11_structure_mark_clears_full_smudge_footprints_and_refinery_hole() {
    let mut sim = Simulation::with_seed(1);
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[SmudgeTypes]\n0=EMPTY\n1=TWO\n2=ONE\n3=DISTANT\n[TWO]\nWidth=2\nHeight=2\n",
        ))
        .unwrap();
    sim.install_resolved_terrain_for_new_map(crate::map::resolved_terrain::test_flat_ground_grid(
        16,
    ));
    let stable_id = 41;
    insert_reservation_building(
        &mut sim,
        stable_id,
        "Americans",
        5,
        5,
        &"3x3Refinery".into(),
        0,
    );

    let mut grid = crate::sim::smudge_grid::SmudgeGrid::new(16, 16);
    for (rx, ry, frame_offset) in [(4, 5, 0), (5, 5, 1), (4, 6, 2), (5, 6, 3)] {
        grid.test_force_set(
            rx,
            ry,
            crate::sim::smudge_grid::SmudgeCell {
                type_id: Some(1),
                frame_offset,
            },
        );
    }
    grid.test_force_set(
        7,
        6,
        crate::sim::smudge_grid::SmudgeCell {
            type_id: Some(2),
            frame_offset: 0,
        },
    );
    grid.test_force_set(
        9,
        9,
        crate::sim::smudge_grid::SmudgeCell {
            type_id: Some(3),
            frame_offset: 0,
        },
    );
    let _ = grid.drain_dirty();
    sim.smudge_grid = Some(grid);

    assert!(matches!(
        sim.try_reveal_entity_with_context(
            stable_id,
            request(5, 5, PlacementEvidence::MarkSucceeded),
            super::UninitContext::new(&rules),
        ),
        RevealOutcome::Revealed { .. }
    ));

    let grid = sim.smudge_grid.as_ref().unwrap();
    for cell in [(4, 5), (5, 5), (4, 6), (5, 6), (7, 6)] {
        assert!(grid.cell(cell.0, cell.1).type_id.is_none(), "cell {cell:?}");
    }
    assert!(
        grid.cell(9, 9).type_id.is_some(),
        "a footprint outside the full refinery rectangle survives"
    );
    let expected = vec![(4, 5), (5, 5), (4, 6), (5, 6), (7, 6)];
    assert_eq!(sim.dirty_cells.tactical_for_test(), expected);
    assert!(sim.dirty_cells.radar().is_empty());
    assert_eq!(sim.dirty_cells.radar_generation(), 0);
    assert_eq!(grid.cell(5, 5).frame_offset, 1);
    assert_eq!(grid.cell(5, 6).frame_offset, 3);
}

/// SmudgePlace6B6080 truncates data to a byte; BuildingMark43F81B decodes
/// that saved byte with current Width, even when it names a later origin.
#[test]
fn wrapped_smudge_data_survives_load_and_controls_building_clear() {
    use crate::map::map_file::MapSmudgeEntry;
    use crate::rules::native_processing::{ProcessedRulesLayers, RulesLayerKind, RulesLayerStack};
    use crate::sim::overlay_grid::OverlayGrid;
    use crate::sim::smudge_grid::SmudgeGrid;

    let Some((root, art)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
        return;
    };
    let mut layers = RulesLayerStack::new(root);
    layers.push(
        RulesLayerKind::GameMode,
        crate::rules::ini_parser::IniFile::from_str("[CRATER11]\nWidth=257\nHeight=3\n"),
    );
    layers.push(
        RulesLayerKind::Scenario,
        crate::rules::ini_parser::IniFile::from_str("[CRATER11]\nHeight=2\n"),
    );
    let processed = layers.process_with_fixed_art(&art).unwrap();
    let saved_rules = save_codec::serialize(&processed).unwrap();
    let rules = crate::rules::ruleset::RuleSet::from_processed_rules(&processed).unwrap();
    let terrain = crate::map::resolved_terrain::test_grid(
        300,
        20,
        crate::map::resolved_terrain::test_smudge_cell,
    );
    let overlay = OverlayGrid::new(300, 20);
    let mut grid = SmudgeGrid::from_map_entries(
        &[
            MapSmudgeEntry {
                type_name: "CRATER11".into(),
                rx: 1,
                ry: 10,
            },
            MapSmudgeEntry {
                type_name: "CRATER01".into(),
                rx: 258,
                ry: 11,
            },
        ],
        &rules.smudge_types,
        &terrain,
        &overlay,
        300,
        20,
    );
    let wide = rules.smudge_types.find_by_name("CRATER11").unwrap();
    assert_eq!(grid.cell(257, 10).type_id, Some(wide));
    assert_eq!(grid.cell(257, 10).frame_offset, 0);
    assert_eq!(grid.cell(256, 10).frame_offset, 255);
    let _ = grid.drain_dirty();
    let mut live = Simulation::with_seed(17);
    live.install_resolved_terrain_for_new_map(terrain.clone());
    live.overlay_grid = Some(overlay);
    live.smudge_grid = Some(grid);
    let bytes = GameSnapshot::save(&live, 0, rules.simulation_config_hash(), "wide-smudge", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    let saved_rules: ProcessedRulesLayers = save_codec::deserialize(&saved_rules).unwrap();
    let rules = crate::rules::ruleset::RuleSet::from_processed_rules(&saved_rules).unwrap();
    let definition = rules.smudge_types.get(wide).unwrap();
    assert_eq!((definition.width, definition.height), (257, 2));
    restored.restore_after_snapshot_load().unwrap();
    restored.rebuild_caches_after_load(terrain, Default::default(), &rules);
    restored
        .restore_map_authority_after_snapshot_load(&rules, &rules.overlay_registry(), None)
        .unwrap();

    // A one-cell Building lands on the later data0. Its decoded rectangle
    // clears an unrelated mark ahead, while the original data0 remains.
    insert_reservation_building(&mut restored, 41, "Americans", 257, 10, &"1x1".into(), 0);
    let rng = restored.scenario_rng.native_state_hex();
    assert!(matches!(
        restored.try_reveal_entity_with_context(
            41,
            request(257, 10, PlacementEvidence::MarkSucceeded),
            super::UninitContext::new(&rules),
        ),
        RevealOutcome::Revealed { .. }
    ));
    let grid = restored.smudge_grid.as_ref().unwrap();
    assert_eq!(grid.cell(1, 10).type_id, Some(wide));
    assert_eq!(grid.cell(256, 10).frame_offset, 255);
    assert!(grid.cell(257, 10).type_id.is_none());
    assert!(grid.cell(258, 11).type_id.is_none());
    assert!(restored.dirty_cells.radar().is_empty());
    assert!(
        restored
            .dirty_cells
            .tactical_for_test()
            .contains(&(258, 11))
    );
    assert_eq!(restored.scenario_rng.native_state_hex(), rng);
}
