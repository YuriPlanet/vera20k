//! Raw cell occupation tests: the ground, deck and air occupation bits that
//! units, infantry, buildings and Fly aircraft mark and clear across reveal,
//! limbo, takeoff and landing.

use super::*;

#[test]
fn gsi_04_12_common_raw_occupation_ground_unit_links_marks_then_unlinks_clears() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);

    let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 0, 128, 128));

    assert!(sim.substrate.occupancy.contains_entity(3, 4, 1));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0x20);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);
    let linked = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationListLinked)
        .expect("object list link event");
    let marked = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationMarked)
        .expect("raw mark event");
    assert!(
        linked < marked,
        "selected object list must link before raw mark"
    );

    sim.lifecycle_test_events.clear();
    let _ = sim.object_conceal(1);

    assert!(!sim.substrate.occupancy.contains_entity(3, 4, 1));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);
    let unlinked = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationListUnlinked)
        .expect("object list unlink event");
    let cleared = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationCleared)
        .expect("raw clear event");
    assert!(
        unlinked < cleared,
        "selected object list must unlink before raw clear"
    );
}

#[test]
fn gsi_04_12_common_raw_occupation_structural_deck_unit_tracks_live_collapse_flags() {
    let mut sim = Simulation::new();
    install_common_raw_terrain(&mut sim, 8, 8, 2, Some((3, 4)));
    insert_entity(&mut sim, 1, EntityCategory::Unit);

    let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 6, 128, 128));

    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0x20);
    assert_eq!(
        sim.substrate
            .occupancy
            .count_on_layer(3, 4, MovementLayer::Ground),
        1,
        "raw deck selection must not reuse the OnBridge object-list selector"
    );

    let _ = sim.object_conceal(1);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);

    {
        // The collapse setter 47E040 clears bit100 before any later receiver.
        let terrain = sim.resolved_terrain.as_mut().expect("resolved terrain");
        let cell = terrain.native_cell_identity((3, 4));
        terrain.write_native_cell_flags(
            cell,
            terrain.native_cell_flags(cell) & !BRIDGE_FLAG_STRUCTURAL,
        );
    }

    insert_entity(&mut sim, 2, EntityCategory::Unit);
    let _ = sim.try_reveal_entity(2, common_raw_request(3, 4, 6, 128, 128));

    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0x20);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);

    let _ = sim.object_conceal(2);

    // Ares 3.0 1004C3E0 (hook 0x00744216) clears the recorded ground too
    // (`Simulation::track_raw_mark_at`). Reveal and Conceal ran on the empty
    // rules.
    let ares = crate::rules::ruleset::RuleSet::empty_for_test().ares_form();
    assert_eq!(
        sim.substrate.raw_cell_occupation.ground_bits(3, 4),
        if ares { 0 } else { 0x20 },
        "height-only clear still targets deck after the collapsed mark used ground"
    );
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);
}

#[test]
fn gsi_04_12_common_raw_occupation_signed_z_marks_ground_on_live_bridge() {
    let mut sim = Simulation::new();
    install_common_raw_terrain(&mut sim, 8, 8, 0, Some((3, 4)));
    insert_entity(&mut sim, 1, EntityCategory::Unit);

    let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 0x80, 128, 128));

    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0x20);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);

    let _ = sim.object_conceal(1);
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);

    insert_entity(&mut sim, 2, EntityCategory::Infantry);
    let _ = sim.try_reveal_entity(2, common_raw_request(3, 4, 0x80, 192, 64));

    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0x04);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);
}

/// Signed level -2 grounds at -207 (`ground_height_leptons`), so its deck
/// starts at 209; an on-bridge Drive unit revealed at the deck level sits
/// there exactly, is marked on the deck and cleared there by height.
#[test]
fn gsi_04_12_common_raw_occupation_signed_ground_pins_mark_and_height_clear() {
    let mut sim = Simulation::new();
    install_common_raw_terrain(&mut sim, 8, 8, 0xFE, Some((3, 4)));
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    {
        let unit = sim.substrate.entities.get_mut(1).unwrap();
        unit.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
        unit.set_on_bridge(true);
    }

    let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 2, 128, 128));

    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0x20);

    let _ = sim.object_conceal(1);

    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);
}

#[test]
fn gsi_04_12_common_raw_occupation_high_nonstructural_unit_keeps_native_stale_ground_bit() {
    let mut sim = Simulation::new();
    install_common_raw_terrain(&mut sim, 8, 8, 2, None);
    insert_entity(&mut sim, 1, EntityCategory::Unit);

    let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 6, 128, 128));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0x20);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);

    let _ = sim.object_conceal(1);

    // Ares 3.0 UnitClass_UnmarkOccupationBits 1004C3E0 (hook 0x00744216)
    // also clears the ground plane UnitClass_MarkOccupationBits recorded
    // (`Simulation::track_raw_mark_at`). Reveal and Conceal ran on the empty
    // rules.
    let ares = crate::rules::ruleset::RuleSet::empty_for_test().ares_form();
    assert_eq!(
        sim.substrate.raw_cell_occupation.ground_bits(3, 4),
        if ares { 0 } else { 0x20 },
        "height-only clear targets deck even though nonstructural mark used ground"
    );
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);
}

#[test]
fn gsi_04_12_common_raw_occupation_infantry_masks_follow_coordinates_and_never_bit_02() {
    let mut sim = Simulation::new();
    let cases = [
        (1, 1, 1, 128, 128, 0x01),
        (2, 2, 1, 64, 64, 0x01),
        (3, 3, 1, 192, 64, 0x04),
        (4, 4, 1, 64, 192, 0x08),
        (5, 5, 1, 192, 192, 0x10),
        (6, 6, 1, 128, 64, 0x01),
        (7, 7, 1, 192, 128, 0x04),
        (8, 8, 1, 128, 192, 0x08),
        (9, 9, 1, 64, 128, 0x01),
    ];

    for &(stable_id, rx, ry, sub_x, sub_y, expected_mask) in &cases {
        insert_entity(&mut sim, stable_id, EntityCategory::Infantry);
        let _ = sim.try_reveal_entity(stable_id, common_raw_request(rx, ry, 0, sub_x, sub_y));
        let bits = sim.substrate.raw_cell_occupation.ground_bits(rx, ry);
        assert_eq!(bits, expected_mask, "coordinate ({sub_x},{sub_y})");
        assert_eq!(bits & 0x02, 0, "GetSubCell never produces raw bit 0x02");
    }
}

/// Infantry Mark skips the receiver (AddContent `0x0047E9EA`, RemoveContent
/// `0x0047EAFE`). `InfantryClass::Unlimbo` writes the sub-cell after Foot's
/// Unlimbo has linked him (`0x0051E0F6..0x0051E10E`), and Walk's Limbo hook
/// (`0x004DB324` -> `0x0075CA69`) clears it before Techno's Limbo unlinks him.
#[test]
fn gsi_04_12_common_raw_occupation_infantry_unlimbo_marks_after_link_and_walk_limbo_clears_first() {
    let mut sim = Simulation::new();
    insert_walker(&mut sim, 1);

    let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 0, 192, 64));
    let linked = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationListLinked)
        .expect("Infantry list link");
    let marked = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationMarked)
        .expect("Infantry raw mark");
    assert!(linked < marked);
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0x04);
    assert_eq!(
        sim.substrate
            .raw_cell_occupation
            .ground_infantry_owner(3, 4),
        Some(sim.substrate.entities.get(1).unwrap().owner())
    );

    sim.lifecycle_test_events.clear();
    let _ = sim.techno_limbo(1);

    assert!(!sim.substrate.occupancy.contains_entity(3, 4, 1));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);
    assert_eq!(
        sim.substrate
            .raw_cell_occupation
            .ground_infantry_owner(3, 4),
        None
    );
    let cleared = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationCleared)
        .expect("Walk Limbo raw clear");
    let unlinked = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationListUnlinked)
        .expect("Infantry list unlink");
    assert!(cleared < unlinked);
}

/// The same Limbo hook (`0x004DB324`) on a Chrono Legionnaire's Teleport
/// (`0x0071A090`) or a grounded Rocketeer's Jumpjet (`0x0054D930`, State 0)
/// clears his sub-cell at his Location. Nothing else would: Infantry Mark
/// clears none (`0x0047EAFE`).
#[test]
fn teleport_and_grounded_jumpjet_limbo_clear_the_infantrymans_sub_cell() {
    for kind in [LocomotorKind::Teleport, LocomotorKind::Jumpjet] {
        let mut sim = Simulation::new();
        insert_entity(&mut sim, 1, EntityCategory::Infantry);
        sim.substrate.entities.get_mut(1).unwrap().locomotor =
            Some(LocomotorState::for_test_kind(kind));
        let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 0, 192, 64));
        assert_eq!(
            sim.substrate.raw_cell_occupation.ground_bits(3, 4),
            0x04,
            "{kind:?}"
        );

        let _ = sim.techno_limbo(1);

        assert!(
            !sim.substrate.occupancy.contains_entity(3, 4, 1),
            "{kind:?}"
        );
        assert_eq!(
            sim.substrate.raw_cell_occupation.ground_bits(3, 4),
            0,
            "{kind:?}"
        );
    }
}

/// Foot4DB324 calls Hover5171C0(0), whose original Head_To_Coord514D10
/// feeds Unit744210. A moving Hover claims that head rather than Location;
/// Stop_Moving516320 retains it, and Foot's disabled current mark cannot
/// release it. The head remains retained while the raw claim is removed.
#[test]
fn hover_limbo_releases_actual_head_claim_and_repeated_limbo_preserves_a_new_resident() {
    use crate::sim::components::DriveCoord;

    // The fixture's entry points run on the empty rules; the direct raw
    // receivers take the same answer.
    let ares = crate::rules::ruleset::RuleSet::empty_for_test().ares_form();
    for (head, cell, deck) in [
        (Some(DriveCoord::cell(3, 3, 0)), (3, 3), false),
        (Some(DriveCoord::cell(3, 3, 416)), (3, 3), true),
        (None, (2, 3), false),
    ] {
        let mut sim = Simulation::new();
        install_common_raw_terrain(&mut sim, 8, 8, 0, deck.then_some(cell));
        insert_entity(&mut sim, 1, EntityCategory::Unit);
        sim.substrate.entities.get_mut(1).unwrap().locomotor =
            Some(LocomotorState::for_test_kind(LocomotorKind::Hover));
        let _ = sim.try_reveal_entity(1, common_raw_request(2, 3, 0, 128, 128));
        let current = DriveCoord::cell(2, 3, 0);
        Simulation::object_raw_receiver_at(
            &mut sim.substrate,
            &sim.path_grid,
            &sim.resolved_terrain,
            1,
            current,
            false,
            ares,
        );
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        entity.set_foot_occupation_enabled(false);
        entity
            .locomotor
            .as_mut()
            .unwrap()
            .hover_runtime_mut()
            .unwrap()
            .set_head(head);
        Simulation::object_raw_receiver_at(
            &mut sim.substrate,
            &sim.path_grid,
            &sim.resolved_terrain,
            1,
            head.unwrap_or(current),
            true,
            ares,
        );
        let bits = |sim: &Simulation| {
            if deck {
                sim.substrate.raw_cell_occupation.deck_bits(cell.0, cell.1)
            } else {
                sim.substrate
                    .raw_cell_occupation
                    .ground_bits(cell.0, cell.1)
            }
        };
        assert_eq!(bits(&sim) & 0x20, 0x20, "accepted Hover head claim");

        let _ = sim.techno_limbo(1);

        assert_eq!(bits(&sim) & 0x20, 0, "actual head must be released");
        assert!(!sim.substrate.occupancy.contains_entity(2, 3, 1));
        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .locomotor
                .as_ref()
                .unwrap()
                .step_head(),
            head,
            "Mark_All(0) preserves the retained head"
        );
        insert_entity(&mut sim, 2, EntityCategory::Unit);
        Simulation::object_raw_receiver_at(
            &mut sim.substrate,
            &sim.path_grid,
            &sim.resolved_terrain,
            2,
            head.unwrap_or(current),
            true,
            ares,
        );
        let _ = sim.techno_limbo(1);
        assert_eq!(
            bits(&sim) & 0x20,
            0x20,
            "already-Limbo object cannot clear a new resident"
        );
    }
}

/// A man placed exactly a deck above a structural bridge's ground is marked
/// on the deck plane (`0x005217C0`) and cleared there by height alone
/// (`0x00521850`). One placed higher is not marked at all: Unlimbo skips its
/// vt+0xF0 above the deck (`0x0051E0F6..0x0051E10E`).
#[test]
fn gsi_04_12_common_raw_occupation_infantry_on_the_deck_marks_and_clears_deck() {
    let mut sim = Simulation::new();
    install_common_raw_terrain(&mut sim, 8, 8, 1, Some((3, 4)));
    insert_walker(&mut sim, 1);

    let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 5, 192, 64));

    assert!(sim.substrate.occupancy.contains_entity(3, 4, 1));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0x04);
    assert_eq!(
        sim.substrate.raw_cell_occupation.deck_infantry_owner(3, 4),
        Some(sim.substrate.entities.get(1).unwrap().owner())
    );
    assert!(
        sim.lifecycle_test_events
            .contains(&LifecycleTestEvent::RawOccupationListLinked)
    );
    assert!(
        sim.lifecycle_test_events
            .contains(&LifecycleTestEvent::RawOccupationMarked)
    );

    let _ = sim.techno_limbo(1);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);
    assert_eq!(
        sim.substrate.raw_cell_occupation.deck_infantry_owner(3, 4),
        None
    );

    insert_walker(&mut sim, 2);
    let _ = sim.try_reveal_entity(2, common_raw_request(3, 4, 6, 192, 64));
    assert!(sim.substrate.occupancy.contains_entity(3, 4, 2));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);
}

#[test]
fn gsi_04_12_common_raw_occupation_high_nonstructural_infantry_retains_mark_plane_asymmetry() {
    let mut sim = Simulation::new();
    install_common_raw_terrain(&mut sim, 8, 8, 2, None);
    insert_walker(&mut sim, 1);

    let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 6, 192, 64));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0x04);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);

    let _ = sim.techno_limbo(1);
    assert_eq!(
        sim.substrate.raw_cell_occupation.ground_bits(3, 4),
        0x04,
        "InfantryClass::Unmark selects the high plane by Z without retesting Flags&0x100"
    );
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);
}

#[test]
fn gsi_04_12_common_raw_occupation_building_foundation_is_ground_only() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Structure);
    {
        let building = sim.substrate.entities.get_mut(1).expect("building");
        building
            .seldom
            .type_facts
            .set_foundation_for_test("2x2".to_string().into());
        building.set_on_bridge(true);
    }

    let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 9, 128, 128));

    for (rx, ry) in [(3, 4), (3, 5), (4, 4), (4, 5)] {
        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(rx, ry), 0x80);
        assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(rx, ry), 0);
        assert_eq!(
            sim.substrate
                .occupancy
                .count_on_layer(rx, ry, MovementLayer::Bridge),
            1
        );
    }

    let _ = sim.object_conceal(1);
    for (rx, ry) in [(3, 4), (3, 5), (4, 4), (4, 5)] {
        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(rx, ry), 0);
        assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(rx, ry), 0);
    }
}

#[test]
fn gsi_04_05_hidden_lifecycle_follows_base_lists_without_expanding_them() {
    let mut sim = Simulation::new();
    sim.session.set_map_width_for_test(16);
    sim.session.set_map_height_for_test(16);
    insert_entity(&mut sim, 1, EntityCategory::Structure);
    {
        let building = sim.substrate.entities.get_mut(1).expect("building");
        building
            .seldom
            .type_facts
            .set_foundation_for_test("2x2".to_string().into());
        let mut profile = building
            .seldom
            .type_facts
            .building_hidden_occupancy()
            .expect("structure constructor profile");
        profile.add_occupy[0] = Some((-1, 0));
        profile.remove_occupy[0] = Some((1, 1));
        building
            .seldom
            .type_facts
            .set_building_hidden_occupancy_for_test(Some(profile));
    }

    let _ = sim.try_reveal_entity(1, common_raw_request(4, 4, 0, 128, 128));
    assert!(sim.substrate.occupancy.contains_entity(5, 5, 1));
    assert!(!sim.substrate.occupancy.contains_entity(3, 4, 1));
    assert_eq!(sim.substrate.hidden_occupation.count(3, 4), 1);
    assert_eq!(sim.substrate.hidden_occupation.count(5, 5), 0);
    let linked = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationListLinked)
        .expect("base lists linked");
    let hidden = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::HiddenOccupationEntered)
        .expect("hidden counters entered");
    assert!(linked < hidden);

    sim.lifecycle_test_events.clear();
    let _ = sim.object_conceal(1);
    assert!(!sim.substrate.occupancy.contains_entity(5, 5, 1));
    assert_eq!(sim.substrate.hidden_occupation.entry_count(), 0);
    let unlinked = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationListUnlinked)
        .expect("base lists unlinked");
    let hidden = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::HiddenOccupationExited)
        .expect("hidden counters exited");
    assert!(unlinked < hidden);
}

/// Mark asks the object's layer (vt+0x78, `0x004D37A6`), which a Drive
/// locomotor answers as Ground whatever VERA's path layer says.
#[test]
fn gsi_04_12_common_raw_occupation_follows_the_layer_query_not_the_path_layer() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 2, EntityCategory::Unit);
    let mut locomotor = LocomotorState::for_test_kind(LocomotorKind::Drive);
    locomotor.set_layer_for_test(MovementLayer::Air);
    sim.substrate.entities.get_mut(2).unwrap().locomotor = Some(locomotor);
    let _ = sim.try_reveal_entity(2, common_raw_request(5, 6, 0, 128, 128));
    assert!(sim.substrate.occupancy.contains_entity(5, 6, 2));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(5, 6), 0x20);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(5, 6), 0);
}

#[test]
fn gsi_04_12_object_raw_occupation_landed_fly_reveal_and_conceal_are_ordered() {
    let mut sim = Simulation::new();
    install_common_raw_terrain(&mut sim, 8, 8, 0, None);
    install_fly_aircraft(&mut sim, 1, SimFixed::from_num(0));

    let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 0, 128, 128));

    assert_eq!(
        sim.substrate
            .occupancy
            .count_on_layer(3, 4, MovementLayer::Ground),
        1
    );
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0x40);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);
    let linked = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationListLinked)
        .expect("landed Fly list link");
    let marked = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationMarked)
        .expect("landed Fly raw mark");
    assert!(linked < marked);

    sim.lifecycle_test_events.clear();
    let _ = sim.object_conceal(1);

    assert!(!sim.substrate.occupancy.contains_entity(3, 4, 1));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);
    let unlinked = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationListUnlinked)
        .expect("landed Fly list unlink");
    let cleared = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationCleared)
        .expect("landed Fly raw clear");
    assert!(unlinked < cleared);
}

#[test]
fn gsi_04_12_object_raw_occupation_airborne_and_non_fly_aircraft_are_excluded() {
    let mut sim = Simulation::new();
    install_common_raw_terrain(&mut sim, 8, 8, 0, None);
    install_fly_aircraft(&mut sim, 1, SimFixed::from_num(1));
    let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 0, 128, 128));
    // SetHeight's Mark pair re-lists it at height 1, in the air.
    sim.set_object_height(
        1,
        1,
        crate::rules::ruleset::RuleSet::empty_for_test(),
        &mut crate::sim::world::FrameEffects::default(),
    );

    insert_entity(&mut sim, 2, EntityCategory::Aircraft);
    sim.substrate.entities.get_mut(2).unwrap().locomotor =
        Some(LocomotorState::for_test_kind(LocomotorKind::Rocket));
    let _ = sim.try_reveal_entity(2, common_raw_request(5, 6, 0, 128, 128));

    for (stable_id, rx, ry) in [(1, 3, 4), (2, 5, 6)] {
        assert!(!sim.substrate.occupancy.contains_entity(rx, ry, stable_id));
        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(rx, ry), 0);
        assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(rx, ry), 0);
    }
}

#[test]
fn gsi_04_12_object_raw_occupation_deck_clear_rechecks_live_structural_state() {
    let mut sim = Simulation::new();
    // Level 1 makes z=5 exactly one deck above ground.
    install_common_raw_terrain(&mut sim, 8, 8, 1, Some((3, 4)));
    install_fly_aircraft(&mut sim, 1, SimFixed::from_num(0));
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .set_on_bridge(true);

    let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 5, 128, 128));
    assert_eq!(
        sim.substrate
            .occupancy
            .count_on_layer(3, 4, MovementLayer::Bridge),
        1
    );
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0x40);

    let _ = sim.object_conceal(1);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);

    install_fly_aircraft(&mut sim, 2, SimFixed::from_num(0));
    sim.substrate
        .entities
        .get_mut(2)
        .unwrap()
        .set_on_bridge(true);
    let _ = sim.try_reveal_entity(2, common_raw_request(3, 4, 5, 128, 128));
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0x40);

    {
        // The collapse setter 47E040 clears bit100 before any later receiver.
        let terrain = sim.resolved_terrain.as_mut().expect("resolved terrain");
        let cell = terrain.native_cell_identity((3, 4));
        terrain.write_native_cell_flags(
            cell,
            terrain.native_cell_flags(cell) & !BRIDGE_FLAG_STRUCTURAL,
        );
    }

    let _ = sim.object_conceal(2);
    assert!(!sim.substrate.occupancy.contains_entity(3, 4, 2));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);
    assert_eq!(
        sim.substrate.raw_cell_occupation.deck_bits(3, 4),
        0x40,
        "generic clear retargets ground after collapse and leaves the deck bit stale"
    );
}

#[test]
fn gsi_04_12_object_raw_occupation_production_fly_tick_unmarks_takeoff_and_marks_landing() {
    let mut sim = Simulation::new();
    install_common_raw_terrain(&mut sim, 8, 8, 0, None);
    install_fly_aircraft(&mut sim, 1, SimFixed::from_num(0));
    let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 0, 128, 128));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0x40);

    {
        let locomotor = sim
            .substrate
            .entities
            .get_mut(1)
            .unwrap()
            .locomotor
            .as_mut()
            .unwrap();

        locomotor.set_fly_target_height(600);
    }
    sim.tick_air_movement_with_cell_lists_one(
        1,
        crate::rules::ruleset::RuleSet::empty_for_test(),
        None,
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap();

    let aircraft = sim.substrate.entities.get(1).unwrap();
    assert!(
        crate::sim::movement::air_movement::current_fly_height(
            aircraft,
            sim.resolved_terrain.as_ref()
        ) > 0
    );
    assert!(
        aircraft.lifecycle.cell_marked(),
        "the post-process Mark transaction completed"
    );
    assert!(!sim.substrate.occupancy.contains_entity(3, 4, 1));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);

    {
        let locomotor = sim
            .substrate
            .entities
            .get_mut(1)
            .unwrap()
            .locomotor
            .as_mut()
            .unwrap();
        locomotor.begin_fly_landing();
        locomotor.set_fly_target_height(0);
    }
    // Supply the matching physical state for this independent landing visit.
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .position
        .set_exact_z_leptons_for_test(Some(1));
    sim.tick_air_movement_with_cell_lists_one(
        1,
        crate::rules::ruleset::RuleSet::empty_for_test(),
        None,
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap();

    let aircraft = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        crate::sim::movement::air_movement::current_fly_height(
            aircraft,
            sim.resolved_terrain.as_ref()
        ),
        0
    );
    assert!(aircraft.lifecycle.cell_marked());
    assert_eq!(
        sim.substrate
            .occupancy
            .count_on_layer(3, 4, MovementLayer::Ground),
        1
    );
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0x40);
}

#[test]
fn gsi_05_05_fly_takeoff_commits_absolute_z_after_remove_process() {
    let mut sim = Simulation::new();
    install_common_raw_terrain(&mut sim, 8, 8, 2, None);
    install_fly_aircraft(&mut sim, 1, SimFixed::from_num(0));
    let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 2, 128, 128));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0x40);

    {
        let locomotor = sim
            .substrate
            .entities
            .get_mut(1)
            .unwrap()
            .locomotor
            .as_mut()
            .unwrap();

        locomotor.set_fly_target_height(600);
    }
    sim.tick_air_movement_with_cell_lists_one(
        1,
        crate::rules::ruleset::RuleSet::empty_for_test(),
        None,
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap();

    let aircraft = sim.substrate.entities.get(1).unwrap();
    let altitude = crate::sim::movement::air_movement::current_fly_height(
        aircraft,
        sim.resolved_terrain.as_ref(),
    );
    assert!(altitude > 0);
    assert_eq!(aircraft.position.exact_z_leptons(), Some(208 + altitude));
    assert!(!sim.substrate.occupancy.contains_entity(3, 4, 1));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);
}

#[test]
fn gsi_05_05_fly_landing_on_bridge_uses_absolute_z_for_deck_put() {
    let mut sim = Simulation::new();
    install_common_raw_terrain(&mut sim, 8, 8, 0, Some((3, 4)));
    install_fly_aircraft(&mut sim, 1, SimFixed::from_num(1));
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .set_on_bridge(true);
    let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 0, 128, 128));
    // SetHeight's Mark pair re-lists it one lepton above the deck, in the air.
    sim.set_object_height(
        1,
        1,
        crate::rules::ruleset::RuleSet::empty_for_test(),
        &mut crate::sim::world::FrameEffects::default(),
    );
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);

    {
        let locomotor = sim
            .substrate
            .entities
            .get_mut(1)
            .unwrap()
            .locomotor
            .as_mut()
            .unwrap();
        locomotor.begin_fly_landing();
        locomotor.set_fly_target_height(0);
    }
    sim.tick_air_movement_with_cell_lists_one(
        1,
        crate::rules::ruleset::RuleSet::empty_for_test(),
        None,
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap();

    let aircraft = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        crate::sim::movement::air_movement::current_fly_height(
            aircraft,
            sim.resolved_terrain.as_ref()
        ),
        0
    );
    assert_eq!(aircraft.position.exact_z_leptons(), Some(416));
    assert_eq!(
        sim.substrate
            .occupancy
            .count_on_layer(3, 4, MovementLayer::Bridge),
        1
    );
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0x40);
}

#[test]
fn gsi_05_05_object_raw_occupation_uses_exact_sloped_ground_z_for_put_and_remove() {
    let mut sim = Simulation::new();
    install_common_raw_terrain(&mut sim, 8, 8, 1, Some((3, 4)));
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut_for_test(3, 4)
        .unwrap()
        .slope_type = 1;
    install_fly_aircraft(&mut sim, 1, SimFixed::from_num(0));
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .set_on_bridge(true);
    let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 0, 64, 192));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0x40);
    sim.remove_entity_occupancy(1);
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);

    // Level 1 plus slope 1 at local X=64 produces exact ground Z 130.
    // Keep coarse Z deliberately above the deck so only exact Object Z can
    // distinguish the two sides of the inclusive 416-lepton threshold.
    {
        let aircraft = sim.substrate.entities.get_mut(1).unwrap();
        aircraft.position.set_z_for_test(0x7f);
        aircraft
            .position
            .set_exact_z_leptons_for_test(Some(130 + 415));
    }
    sim.add_entity_occupancy(1);
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0x40);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);
    sim.remove_entity_occupancy(1);
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);

    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .position
        .set_exact_z_leptons_for_test(Some(130 + 416));
    sim.add_entity_occupancy(1);
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0x40);
    sim.remove_entity_occupancy(1);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);
}

#[test]
fn gsi_05_05_mapless_fly_uses_dummy_ground_then_bridge_height() {
    let mut sim = Simulation::new();
    install_fly_aircraft(&mut sim, 1, SimFixed::from_num(100));
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .set_on_bridge(true);
    let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 2, 128, 128));
    // A mapless reveal keeps no exact Z; SetHeight writes one over the Dummy
    // cell's ground zero and the OnBridge deck.
    Simulation::set_object_height_unmarked(&mut sim.substrate, &sim.resolved_terrain, 1, 100);
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .position
            .exact_z_leptons(),
        Some(416 + 100)
    );

    sim.tick_air_movement_with_cell_lists_one(
        1,
        crate::rules::ruleset::RuleSet::empty_for_test(),
        None,
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap();

    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .position
            .exact_z_leptons(),
        Some(416 + 100),
        "Fly's GetHeight reads the same dummy ground back, so level flight writes nothing"
    );
}

#[test]
fn gsi_04_07_damage_air_spatial_entry_crossing_and_exit_keep_vector_order() {
    let mut sim = Simulation::new();
    sim.session.set_map_width_for_test(40);
    sim.session.set_map_height_for_test(40);
    install_common_raw_terrain(&mut sim, 40, 40, 0, None);
    // Foot Unlimbo4D72B2 adds ConsideredAircraft only above the native
    // high-flight threshold (208 leptons); a Reveal without rules commits no
    // exact Z, so the fixture lifts both aircraft and adds them in Unlimbo's
    // order (`unlimbo_adds_a_high_flying_considered_aircraft_to_the_air_tracker`
    // covers that admission).
    install_fly_aircraft(&mut sim, 20, SimFixed::from_num(416));
    install_fly_aircraft(&mut sim, 10, SimFixed::from_num(416));

    let _ = sim.try_reveal_entity(20, common_raw_request(2, 4, 4, 128, 128));
    let _ = sim.try_reveal_entity(10, common_raw_request(3, 4, 4, 128, 128));
    for id in [20, 10] {
        sim.set_object_height(
            id,
            416,
            crate::rules::ruleset::RuleSet::empty_for_test(),
            &mut crate::sim::world::FrameEffects::default(),
        );
        Simulation::aircraft_tracker_add(&sim.session, &mut sim.substrate, id);
    }
    let first = sim.substrate.entities.get(20).unwrap();
    let second = sim.substrate.entities.get(10).unwrap();
    assert!(first.air_spatial_bucket().is_some());
    assert_eq!(first.air_spatial_bucket(), second.air_spatial_bucket());
    assert!(
        first.air_spatial_enter_order() < second.air_spatial_enter_order(),
        "same-bucket vector retains append order, not stable-ID order"
    );
    let first_order = first.air_spatial_enter_order();
    let shared_bucket = second.air_spatial_bucket();
    let second_order = second.air_spatial_enter_order();

    sim.tick_air_movement_with_cell_lists_one(
        20,
        crate::rules::ruleset::RuleSet::empty_for_test(),
        None,
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    assert_eq!(
        sim.substrate
            .entities
            .get(20)
            .unwrap()
            .air_spatial_enter_order(),
        first_order,
        "Fly's temporary cell-list transaction is not an air-vector re-entry"
    );

    // The tracker follows a cell change of a Fly whose current speed is not
    // zero (`0x004CD60E..0x004CD65F`); without a type Speed it stays put.
    let aircraft = sim.substrate.entities.get_mut(20).unwrap();
    aircraft.position.set_rx_for_test(12);
    aircraft
        .locomotor
        .as_mut()
        .unwrap()
        .fly_runtime_mut()
        .unwrap()
        .set_current_speed_for_test(SimFixed::from_num(1));
    sim.tick_air_movement_with_cell_lists_one(
        20,
        crate::rules::ruleset::RuleSet::empty_for_test(),
        None,
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    let crossed = sim.substrate.entities.get(20).unwrap();
    assert_ne!(crossed.air_spatial_bucket(), shared_bucket);
    assert!(crossed.air_spatial_enter_order() > second_order);

    // FootClass::Limbo removes the tracked aircraft (`0x004DB3AA`); its
    // Mark(UP) does not.
    let _ = sim.techno_limbo(20);
    let exited = sim.substrate.entities.get(20).unwrap();
    assert_eq!(exited.air_spatial_bucket(), None);
    assert_eq!(exited.air_spatial_enter_order(), 0);
}

/// Foot Unlimbo's AircraftTracker admission (`0x004D72B2..0x004D72DB`) is
/// not Fly's alone: a high-flying object (vt+0x54) whose type sets
/// ConsideredAircraft (TechnoType `+0xD96`) enters the tracker at Reveal,
/// here a Jumpjet VehicleType. Without that key it does not. Mark itself
/// never adds anything.
#[test]
fn unlimbo_adds_a_high_flying_considered_aircraft_to_the_air_tracker() {
    for (considered_aircraft, tracked) in [("yes", true), ("no", false)] {
        let rules = crate::rules::ruleset::RuleSet::from_ini(
            &crate::rules::ini_parser::IniFile::from_str(&format!(
                "[VehicleTypes]\n0=JJET\n\
                 [JJET]\nStrength=100\nSpeed=14\nConsideredAircraft={considered_aircraft}\n\
                 Locomotor={{92612C46-F71F-11d1-AC9F-006008055BB5}}\n",
            )),
        )
        .expect("Jumpjet VehicleType rules");
        let mut sim = Simulation::new();
        sim.session.set_map_width_for_test(40);
        sim.session.set_map_height_for_test(40);
        install_common_raw_terrain(&mut sim, 40, 40, 0, None);
        insert_entity(&mut sim, 30, EntityCategory::Unit);
        let type_ref = sim.interner.intern("JJET");
        let entity = sim.substrate.entities.get_mut(30).unwrap();
        entity.set_type_ref_for_test(type_ref);
        // UnitType's floor clamp keeps the level-4 input, 416 leptons up.
        entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Jumpjet));

        let _ = sim.try_reveal_entity_with_context(
            30,
            common_raw_request(3, 4, 4, 128, 128),
            super::UninitContext::new(&rules),
        );

        let jumpjet = sim.substrate.entities.get(30).unwrap();
        assert!(jumpjet.lifecycle.cell_marked());
        assert_eq!(jumpjet.air_spatial_bucket().is_some(), tracked);
        assert!(!sim.substrate.occupancy.contains_entity(3, 4, 30));
    }
}
