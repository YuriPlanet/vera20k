//! Building base reservation tests: the reserved rectangle a structure marks
//! on reveal, its repair after limbo, and the map-edge gate that owner
//! changes refresh.

use super::*;

fn packed_reservation_test_coord(x: i32, y: i32) -> u32 {
    u32::from(x as i16 as u16) | (u32::from(y as i16 as u16) << 16)
}

fn gsi_04_16_edge_gate_rules() -> crate::rules::ruleset::RuleSet {
    let ini = crate::rules::ini_parser::IniFile::from_str(
        "[BuildingTypes]\n0=GACNST\n1=CAOILD\n\
         [GACNST]\nStrength=1000\nFactory=BuildingType\n\
         [CAOILD]\nStrength=1000\n",
    );
    crate::rules::ruleset::RuleSet::from_ini(&ini).expect("edge-gate fixture rules")
}

fn gsi_04_16_dustbowl_bounds() -> crate::sim::cell_rect::PlayfieldBounds {
    crate::sim::cell_rect::PlayfieldBounds {
        base: 70,
        off_fc: 2,
        off_100: 8,
        off_104: 65,
        off_108: 62,
    }
}

#[test]
fn gsi_04_05_reservation_successful_reveal_marks_expanded_rect_after_lists() {
    let mut sim = Simulation::new();
    let owner = insert_reservation_building(&mut sim, 101, "Americans", 10, 20, &"2x3".into(), 1);

    assert!(matches!(
        sim.try_reveal_entity(101, request(10, 20, PlacementEvidence::MarkSucceeded)),
        RevealOutcome::Revealed { .. }
    ));
    for x in 9..=12 {
        for y in 19..=23 {
            assert_eq!(sim.substrate.base_reservations.raw_mask(None, x, y), 1);
        }
    }
    assert_eq!(sim.substrate.base_reservations.raw_mask(None, 8, 20), 0);
    assert_eq!(sim.substrate.base_reservations.raw_mask(None, 13, 20), 0);
    let house = sim.houses.get(&owner).expect("reservation owner");
    assert_eq!(house.base_reservation().bounds(), (9, 19, 4, 5));
    assert_eq!(
        house.base_reservation().perimeter_cells(),
        &[
            packed_reservation_test_coord(9, 19),
            packed_reservation_test_coord(9, 20),
            packed_reservation_test_coord(9, 21),
            packed_reservation_test_coord(9, 22),
            packed_reservation_test_coord(9, 23),
            packed_reservation_test_coord(10, 19),
            packed_reservation_test_coord(10, 23),
            packed_reservation_test_coord(11, 19),
            packed_reservation_test_coord(11, 23),
            packed_reservation_test_coord(12, 19),
            packed_reservation_test_coord(12, 20),
            packed_reservation_test_coord(12, 21),
            packed_reservation_test_coord(12, 22),
            packed_reservation_test_coord(12, 23),
        ]
    );

    let cell_marked = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::CellMarked)
        .unwrap();
    let reservation_marked = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::BaseReservationMarked)
        .unwrap();
    let display = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RevealDisplayBoundary)
        .unwrap();
    assert!(cell_marked < reservation_marked && reservation_marked < display);
}

#[test]
fn gsi_04_16_dustbowl_conyard_unlimbo_uses_local_size_edge() {
    let mut sim = Simulation::new();
    sim.playfield_bounds = Some(gsi_04_16_dustbowl_bounds());
    let rules = gsi_04_16_edge_gate_rules();
    let owner = sim.interner.intern("Americans");
    let mut house = HouseState::new(owner, 0, None, true, 0, 10);
    house.set_base_center_for_test(Some((70, 116)));
    sim.houses.register(house);

    let conyard = sim
        .spawn_object("GACNST", "Americans", 69, 115, 0, &rules)
        .expect("GACNST reveals");
    assert!(sim.production.is_primary_factory(conyard));
    let house = sim.houses.get(&owner).expect("launch house");
    assert_eq!(house.base_center(), Some((70, 116)));
    assert_eq!(house.waypoint_edge(), 2);
}

#[test]
fn gsi_04_16_committed_structure_owner_change_refreshes_new_house_edge() {
    let mut sim = Simulation::new();
    sim.playfield_bounds = Some(gsi_04_16_dustbowl_bounds());
    let rules = gsi_04_16_edge_gate_rules();
    let old_owner = sim.interner.intern("Americans");
    let new_owner = sim.interner.intern("Soviets");
    sim.houses
        .register(HouseState::new(old_owner, 0, None, true, 0, 10));
    let mut new_house = HouseState::new(new_owner, 1, None, false, 0, 10);
    new_house.set_base_center_for_test(Some((68, 114)));
    sim.houses.register(new_house);
    let conyard = sim
        .spawn_object("GACNST", "Americans", 69, 115, 0, &rules)
        .expect("GACNST reveals");

    sim.change_owner_with_rules(conyard, new_owner, &rules, None, &mut Default::default());

    assert!(sim.production.is_primary_factory(conyard));
    let new_house = sim.houses.get(&new_owner).expect("new owner house");
    assert_eq!(new_house.base_center(), Some((68, 114)));
    assert_eq!(new_house.waypoint_edge(), 2);
}

#[test]
fn gsi_04_16_caoild_reveal_and_owner_change_preserve_waypoint_edges() {
    let mut sim = Simulation::new();
    sim.playfield_bounds = Some(gsi_04_16_dustbowl_bounds());
    let rules = gsi_04_16_edge_gate_rules();
    let old_owner = sim.interner.intern("Americans");
    let new_owner = sim.interner.intern("Soviets");
    let mut old_house = HouseState::new(old_owner, 0, None, true, 0, 10);
    old_house.set_waypoint_edge_for_test(1);
    sim.houses.register(old_house);
    let mut new_house = HouseState::new(new_owner, 1, None, false, 0, 10);
    new_house.set_waypoint_edge_for_test(3);
    sim.houses.register(new_house);

    let oil = sim
        .spawn_object("CAOILD", "Americans", 69, 115, 0, &rules)
        .expect("CAOILD reveals");
    assert_eq!(sim.houses.get(&old_owner).unwrap().waypoint_edge(), 1);

    sim.change_owner_with_rules(oil, new_owner, &rules, None, &mut Default::default());

    assert_eq!(sim.houses.get(&new_owner).unwrap().waypoint_edge(), 3);
}

#[test]
fn gsi_04_05_reservation_failed_reveal_never_writes() {
    let mut sim = Simulation::new();
    insert_reservation_building(&mut sim, 102, "Americans", 10, 20, &"2x2".into(), 1);

    assert_eq!(
        sim.try_reveal_entity(102, request(10, 20, PlacementEvidence::MarkFailed)),
        RevealOutcome::Failed(RevealFailure::MarkFailed)
    );
    assert_eq!(sim.substrate.base_reservations.entries().count(), 0);
    assert_eq!(sim.substrate.base_reservations.dummy_mask(), 0);
    assert!(
        !sim.lifecycle_test_events
            .contains(&LifecycleTestEvent::BaseReservationMarked)
    );
}

#[test]
fn gsi_04_05_reservation_clear_removes_perimeter_but_retains_bounds() {
    let mut sim = Simulation::new();
    let owner = insert_reservation_building(&mut sim, 111, "Americans", 10, 10, &"1x1".into(), 1);
    let _ = sim.try_reveal_entity(111, request(10, 10, PlacementEvidence::MarkSucceeded));
    assert_eq!(
        sim.houses
            .get(&owner)
            .unwrap()
            .base_reservation()
            .perimeter_cells()
            .len(),
        8
    );

    assert_eq!(sim.techno_limbo(111), super::ConcealOutcome::Concealed);
    let state = sim.houses.get(&owner).unwrap().base_reservation();
    assert_eq!(state.bounds(), (9, 9, 3, 3));
    assert!(state.perimeter_cells().is_empty());
    assert_eq!(sim.substrate.base_reservations.raw_mask(None, 10, 10), 0);
}

#[test]
fn gsi_04_05_reservation_limbo_clears_before_unlink_repairs_overlap_and_preserves_other_house() {
    let mut sim = Simulation::new();
    insert_reservation_building(&mut sim, 103, "Americans", 10, 10, &"1x1".into(), 1);
    let other = sim.interner.intern("Russians");
    sim.houses.get_or_register_for_test(other, || {
        crate::sim::house_state::HouseState::new(other, 1, None, false, 0, 10)
    });
    insert_reservation_building(&mut sim, 104, "Americans", 12, 10, &"1x1".into(), 1);
    let _ = sim.try_reveal_entity(103, request(10, 10, PlacementEvidence::MarkSucceeded));
    let _ = sim.try_reveal_entity(104, request(12, 10, PlacementEvidence::MarkSucceeded));
    sim.substrate.base_reservations.reserve(None, 9, 10, 1);

    sim.lifecycle_test_events.clear();
    assert_eq!(sim.techno_limbo(103), super::ConcealOutcome::Concealed);

    assert_eq!(
        sim.substrate.base_reservations.raw_mask(None, 9, 10),
        1 << 1,
        "AND-not clears only the leaving owner's bit"
    );
    assert_eq!(
        sim.substrate.base_reservations.raw_mask(None, 11, 10) & 1,
        1,
        "neighbor repair restores the same-house overlap"
    );
    let cleared = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::BaseReservationCleared)
        .unwrap();
    let unlinked = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationListUnlinked)
        .unwrap();
    assert!(cleared < unlinked);
}

#[test]
fn gsi_04_05_reservation_repair_scan_reaches_asymmetric_high_edge() {
    assert_eq!(
        super::lifecycle::building_base_reservation_repair_rect(10, 20, 4, 4, 1),
        crate::sim::cell_rect::CellRect::new(8, 18, 9, 9),
        "half-open repair bounds are [8,17) x [18,27)"
    );
    assert_eq!(
        super::lifecycle::building_base_reservation_repair_rect(10, 20, 6, 6, -1),
        crate::sim::cell_rect::CellRect::new(12, 22, 1, 1),
        "signed negative spacing yields [12,13) x [22,23)"
    );

    let mut sim = Simulation::new();
    insert_reservation_building(&mut sim, 107, "Americans", 10, 20, &"4x4".into(), 1);
    insert_reservation_building(&mut sim, 108, "Americans", 16, 20, &"1x1".into(), 2);
    let _ = sim.try_reveal_entity(107, request(10, 20, PlacementEvidence::MarkSucceeded));
    let _ = sim.try_reveal_entity(108, request(16, 20, PlacementEvidence::MarkSucceeded));
    assert_eq!(sim.substrate.base_reservations.raw_mask(None, 14, 20), 1);

    assert_eq!(sim.techno_limbo(107), super::ConcealOutcome::Concealed);
    assert_eq!(
        sim.substrate.base_reservations.raw_mask(None, 14, 20),
        1,
        "the x=16 neighbor, omitted by the old [8,16) scan, re-marks its overlap"
    );
}

#[test]
fn gsi_04_05_reservation_origins_truncate_after_first_spacing_subtraction() {
    assert_eq!(
        super::lifecycle::building_base_reservation_rect(0, 0, &"1x1".into(), 32_769),
        crate::sim::cell_rect::CellRect::new(32_767, 32_767, 65_539, 65_539)
    );
    assert_eq!(
        super::lifecycle::building_base_reservation_repair_rect(0, 0, 1, 1, 32_769),
        crate::sim::cell_rect::CellRect::new(-2, -2, 163_846, 163_846),
        "repair subtracts spacing from the already signed-16-truncated primary start"
    );
}

#[test]
fn gsi_04_05_reservation_repair_replays_multicell_neighbor_for_every_hit() {
    let mut sim = Simulation::new();
    insert_reservation_building(&mut sim, 109, "Americans", 10, 10, &"1x1".into(), 1);
    insert_reservation_building(&mut sim, 110, "Americans", 12, 10, &"2x2".into(), 1);
    let _ = sim.try_reveal_entity(109, request(10, 10, PlacementEvidence::MarkSucceeded));
    let _ = sim.try_reveal_entity(110, request(12, 10, PlacementEvidence::MarkSucceeded));

    sim.lifecycle_test_events.clear();
    assert_eq!(sim.techno_limbo(109), super::ConcealOutcome::Concealed);
    assert_eq!(
        sim.lifecycle_test_events
            .iter()
            .filter(|event| **event == LifecycleTestEvent::BaseReservationMarked)
            .count(),
        4,
        "the 2x2 neighbor is invoked immediately once for each occupied repair cell"
    );
}

#[test]
fn gsi_04_05_reservation_processed_foundation_precedes_metadata_and_lifecycle_mark() {
    use crate::rules::art_data::ArtRegistry;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;

    let ini = IniFile::from_str(
        "[AI]\nAIBaseSpacing=2\n\
         [BuildingTypes]\n0=GACNST\n1=ONECNST\n\
         [GACNST]\nUndeploysInto=AMCV\n\
         [ONECNST]\nUndeploysInto=AMCV\n",
    );
    let constructor_only =
        RuleSet::from_ini(&ini).expect("split rules-side construction-yard data");
    assert_eq!(
        constructor_only
            .object("GACNST")
            .unwrap()
            .base_reservation_spacing,
        None,
        "the provisional Rules-only 1x1 foundation is ineligible"
    );

    let art = IniFile::from_str(
        "[GACNST]\nFoundation=4x4\n\
         [ONECNST]\nFoundation=1x1\n",
    );
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art)
        .expect("ART is fixed during native BuildingType read");
    rules.install_art_data(ArtRegistry::from_ini(&art));
    let gacnst = rules.object("GACNST").unwrap();
    assert_eq!(gacnst.foundation, "4x4");
    assert_eq!(gacnst.base_reservation_spacing, Some(2));
    assert_eq!(
        rules.object("ONECNST").unwrap().base_reservation_spacing,
        None,
        "an effective ART 1x1 undeployer remains ineligible"
    );
    let foundation = gacnst.foundation.clone();
    let spacing = gacnst.base_reservation_spacing.unwrap();

    let mut sim = Simulation::new();
    insert_reservation_building(&mut sim, 106, "Americans", 30, 40, &foundation, spacing);
    assert!(matches!(
        sim.try_reveal_entity(106, request(30, 40, PlacementEvidence::MarkSucceeded)),
        RevealOutcome::Revealed { .. }
    ));
    assert_eq!(sim.substrate.base_reservations.raw_mask(None, 28, 38), 1);
    assert_eq!(sim.substrate.base_reservations.raw_mask(None, 35, 45), 1);
    assert_eq!(sim.substrate.base_reservations.raw_mask(None, 36, 45), 0);
}
