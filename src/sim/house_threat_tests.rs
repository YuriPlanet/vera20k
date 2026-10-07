//! Native kernel/index execution plus production lifecycle integration.
//! Lifecycle fixtures supply accepted type and physical-position prestates;
//! they do not claim a full native scenario run. Arithmetic expectations come
//! from original56BC50/4FA2E0 execution, never from this Rust kernel.
use super::*;
use crate::sim::house_state::HouseState;
use crate::sim::world::{ConcealOutcome, PlacementEvidence};

fn packet() -> serde_json::Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/astar_threat_inputs.json",
    ))
    .unwrap()
}

fn signed(value: &serde_json::Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

#[test]
fn original_signed_indices_and_all_nine_slot_adjustments() {
    let packet = packet();
    assert!(packet["code_unchanged"].as_bool().unwrap());
    let spatial = &packet["spatial"];
    let indices = spatial["indices"].as_array().unwrap();
    assert_eq!(indices.len(), 13);
    for row in indices {
        let cell = (
            signed(&row["coord"][0]) as i16,
            signed(&row["coord"][1]) as i16,
        );
        assert_eq!(
            HouseSpatialThreat::index(cell),
            signed(&row["index"]),
            "{row}"
        );
    }
    let cell = (75, 75);
    let center = indices
        .iter()
        .find(|row| row["coord"] == serde_json::json!([75, 75]))
        .unwrap();
    let center = signed(&center["index"]);
    let cases = spatial["adjustments"].as_array().unwrap();
    assert_eq!(cases.len(), 42);
    for row in cases {
        assert!(row["canaries_intact"].as_bool().unwrap());
        let initial = signed(&row["initial"]);
        let mut grid = HouseSpatialThreat::default();
        grid.0.fill(initial);
        grid.adjust(cell, signed(&row["amount"])).unwrap();
        let offsets = row["offsets"].as_array().unwrap();
        for (index, actual) in grid.values().iter().enumerate() {
            let native_slot = offsets
                .iter()
                .position(|offset| center + signed(offset) == index as i32);
            let expected = native_slot.map_or(initial, |slot| signed(&row["after"][slot]));
            assert_eq!(
                *actual, expected,
                "native adjustment {row}, padded index {index}"
            );
        }
    }
}

fn fixture(
    threat: i32,
) -> (
    Simulation,
    RuleSet,
    crate::map::overlay_types::OverlayTypeRegistry,
) {
    fixture_with_additions(threat, "", &crate::rules::ini_parser::IniFile::from_str(""))
}

fn fixture_with_additions(
    threat: i32,
    additions: &str,
    art: &crate::rules::ini_parser::IniFile,
) -> (
    Simulation,
    RuleSet,
    crate::map::overlay_types::OverlayTypeRegistry,
) {
    let mut extra = format!(
        "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=400\nSpeed=6\nSpeedType=Track\nMovementZone=Normal\nThreatPosed={threat}\nThreatAvoidanceCoefficient=0.5\nLocomotor={{4A582741-9839-11d1-B709-00A024DDAFD1}}\n[CABHUT]\nThreatPosed=7\n"
    );
    extra.push_str(additions);
    let (mut sim, rules, registry) =
        crate::sim::world::entry_test_fixture::fixture_with_rules_and_fixed_art(&extra, art);
    let owners: Vec<_> = ["Americans", "Russians", "Other"]
        .into_iter()
        .map(|name| {
            let owner = sim.interner.intern(name);
            sim.houses
                .insert(owner, HouseState::new(owner, 0, None, true, 0, 10));
            owner
        })
        .collect();
    sim.session.house_order = owners;
    sim.session.game_mode_nonzero = true;
    (sim, rules, registry)
}

fn actor(sim: &mut Simulation, rules: &RuleSet, cell: (u16, u16)) -> u64 {
    sim.spawn_object("MTNK", "Americans", cell.0, cell.1, 0, rules)
        .unwrap()
}

fn threat(sim: &Simulation, owner: &str, cell: (i16, i16)) -> i32 {
    sim.house_threat_at_cell(sim.interner.get(owner).unwrap(), cell)
        .unwrap()
}

#[test]
fn admitted_unlimbo_publishes_once_and_refusals_preserve_native_uninitialized_domain() {
    let (mut sim, rules, registry) = fixture(7);
    let id = sim
        .construct_object_limbo_at_height("MTNK", "Americans", 13, 15, 0, 0, &rules)
        .unwrap();
    assert_eq!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .cached_spatial_threat(),
        None
    );
    let rng = sim.rng_state();
    for placement in [
        PlacementEvidence::RejectedEarly,
        PlacementEvidence::MarkFailed,
    ] {
        assert_eq!(
            sim.reveal_constructed_object_at_height(id, 13, 15, 0, 0, placement, &rules),
            None
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .cached_spatial_threat(),
            None
        );
        assert!(sim.houses.values().all(|house| {
            house
                .spatial_threat_values()
                .iter()
                .all(|value| *value == 0)
        }));
    }
    assert_eq!(
        sim.reveal_constructed_object_at_height(
            id,
            13,
            15,
            0,
            0,
            PlacementEvidence::EvaluateMark,
            &rules
        ),
        Some(id)
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .cached_spatial_threat(),
        Some(7)
    );
    assert_eq!(threat(&sim, "Americans", (13, 15)), 0);
    assert_eq!(threat(&sim, "Russians", (13, 15)), 7);
    assert_eq!(sim.rng_state(), rng, "threat publication draws no RNG");
    assert_eq!(
        sim.techno_limbo_with_rules(id, &rules, Some(&registry)),
        ConcealOutcome::Concealed
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .cached_spatial_threat(),
        Some(0)
    );
    assert!(sim.houses.values().all(|house| {
        house
            .spatial_threat_values()
            .iter()
            .all(|value| *value == 0)
    }));
    assert_eq!(
        sim.techno_limbo_with_rules(id, &rules, Some(&registry)),
        ConcealOutcome::AlreadyConcealed
    );
}

#[test]
fn foot_bucket_transition_uses_old_history_then_refreshes_cached_live_threat() {
    let (mut sim, rules, registry) = fixture(7);
    let id = actor(&mut sim, &rules, (13, 15));
    let (_, changed_rules, _) = fixture(100);
    let before = sim
        .houses
        .values()
        .map(|house| house.spatial_threat_values().to_vec())
        .collect::<Vec<_>>();
    // Supplied physical-position prestate before the real PerCell reason2.
    sim.substrate.entities.get_mut(id).unwrap().position.rx = 14;
    sim.per_cell_process(
        id,
        crate::sim::movement::PerCellReason::Arrival,
        Some(&changed_rules),
        Some(&registry),
    )
    .unwrap();
    assert_eq!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .cached_spatial_threat(),
        Some(7)
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .navigation
            .neighbor_state
            .cell(),
        (14, 15)
    );
    assert_eq!(
        sim.houses
            .values()
            .map(|house| house.spatial_threat_values().to_vec())
            .collect::<Vec<_>>(),
        before
    );
    sim.substrate.entities.get_mut(id).unwrap().position.rx = 16;
    sim.per_cell_process(
        id,
        crate::sim::movement::PerCellReason::Arrival,
        Some(&changed_rules),
        Some(&registry),
    )
    .unwrap();
    assert_eq!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .cached_spatial_threat(),
        Some(100)
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .navigation
            .neighbor_state
            .cell(),
        (16, 15)
    );
    assert_eq!(threat(&sim, "Russians", (16, 15)), 100);
    assert_eq!(
        sim.techno_limbo_with_rules(id, &changed_rules, Some(&registry)),
        ConcealOutcome::Concealed
    );
    assert!(sim.houses.values().all(|house| {
        house
            .spatial_threat_values()
            .iter()
            .all(|value| *value == 0)
    }));
}

#[test]
fn owner_transfer_removes_old_contribution_before_recomputing_for_new_house() {
    let (mut sim, rules, _) = fixture(7);
    let id = actor(&mut sim, &rules, (13, 15));
    let russians = sim.interner.get("Russians").unwrap();
    let rng = sim.rng_state();
    sim.change_owner_with_rules(id, russians, &rules, None);
    assert_eq!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .cached_spatial_threat(),
        Some(7)
    );
    assert_eq!(threat(&sim, "Americans", (13, 15)), 7);
    assert_eq!(threat(&sim, "Russians", (13, 15)), 0);
    assert_eq!(threat(&sim, "Other", (13, 15)), 7);
    assert_eq!(sim.rng_state(), rng);
}

#[test]
fn diplomacy_rebuild_preserves_native_foot_and_building_filter_asymmetry() {
    let (mut sim, rules, _) = fixture(7);
    actor(&mut sim, &rules, (13, 15));
    let russian = sim.interner.get("Russians").unwrap();
    let mut alliances = HouseAllianceMap::new();
    alliances
        .entry("RUSSIANS".into())
        .or_default()
        .insert("AMERICANS".into());
    sim.install_house_alliances(alliances.clone(), &rules);
    assert_eq!(
        threat(&sim, "Russians", (13, 15)),
        0,
        "controlled Foot rebuild excludes ally"
    );
    let house = sim.houses.get_mut(&russian).unwrap();
    house.is_human = false;
    house.player_control = false;
    sim.rebuild_house_spatial_threat(russian, &rules);
    assert_eq!(
        threat(&sim, "Russians", (13, 15)),
        7,
        "AI Foot rebuild includes ally"
    );
    let american = sim.interner.get("Americans").unwrap();
    let building = sim
        .spawn_object("CABHUT", "Americans", 18, 14, 0, &rules)
        .unwrap();
    assert_eq!(
        sim.substrate
            .entities
            .get(building)
            .unwrap()
            .cached_spatial_threat(),
        Some(7)
    );
    sim.rebuild_house_spatial_threat(american, &rules);
    assert_eq!(
        threat(&sim, "Americans", (18, 14)),
        7,
        "Building rebuild includes own house"
    );
    let saved = serde_json::to_vec(sim.houses.get(&american).unwrap()).unwrap();
    let restored: HouseState = serde_json::from_slice(&saved).unwrap();
    assert_eq!(
        restored.spatial_threat_values(),
        sim.houses.get(&american).unwrap().spatial_threat_values()
    );
    assert!(serde_json::from_value::<HouseSpatialThreat>(serde_json::json!([0])).is_err());
}

#[test]
#[should_panic(expected = "required native Techno+508 read before an admitted threat publication")]
fn required_read_of_dead_unlimbo_uninitialized_contribution_faults_explicitly() {
    let (mut sim, rules, _) = fixture(7);
    let id = sim
        .construct_object_limbo_at_height("MTNK", "Americans", 13, 15, 0, 0, &rules)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .lifecycle
        .object_alive = false;
    assert_eq!(
        sim.reveal_constructed_object_at_height(
            id,
            13,
            15,
            0,
            0,
            PlacementEvidence::EvaluateMark,
            &rules
        ),
        Some(id)
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .cached_spatial_threat(),
        None
    );
    sim.spatial_threat_before_limbo(id, &rules, None);
}

#[test]
fn invalid_grid_dependencies_fail_before_mutation() {
    let mut grid = HouseSpatialThreat::default();
    grid.adjust((75, 75), 7).unwrap();
    let before = grid.values().to_vec();
    for cell in [(i16::MIN, i16::MIN), (i16::MAX, i16::MAX), (0, -8)] {
        assert!(grid.adjust(cell, 7).is_err());
        assert_eq!(grid.values(), before);
    }
    assert!(grid.at_padded_index(-1).is_err());
    assert!(grid.at_padded_index(LENGTH as i32).is_err());
    let (mut sim, rules, _) = fixture(7);
    actor(&mut sim, &rules, (13, 15));
    let map = sim.resolved_terrain.take();
    let before = sim.state_hash();
    assert!(
        sim.house_threat_at_cell(sim.interner.get("Russians").unwrap(), (13, 15))
            .is_err()
    );
    assert_eq!(sim.state_hash(), before);
    sim.resolved_terrain = map;
}

#[test]
fn snapshot_fixup_retains_map_and_initialized_contribution_without_rebuild() {
    let (mut sim, rules, _) = fixture(7);
    let id = actor(&mut sim, &rules, (13, 15));
    let russian = sim.interner.get("Russians").unwrap();
    // Explicit retained stream prestate: a native map may differ from a fresh
    // census because same-bucket motion and diplomacy rebuild have distinct
    // update rules. Neither native Load nor Rust fixup rebuilds this authority.
    sim.houses
        .get_mut(&russian)
        .unwrap()
        .adjust_spatial_threat((24, 16), 19)
        .unwrap();
    let maps = sim
        .houses
        .iter()
        .map(|(&id, h)| (id, h.spatial_threat_values().to_vec()))
        .collect::<Vec<_>>();
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "retained threat", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(
        restored
            .houses
            .iter()
            .map(|(&id, h)| (id, h.spatial_threat_values().to_vec()))
            .collect::<Vec<_>>(),
        maps
    );
    assert_eq!(
        restored
            .substrate
            .entities
            .get(id)
            .unwrap()
            .cached_spatial_threat(),
        Some(7)
    );
    assert_eq!(
        restored
            .substrate
            .entities
            .get(id)
            .unwrap()
            .navigation
            .path_threat_coefficient(),
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .navigation
            .path_threat_coefficient()
    );
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(
        restored
            .houses
            .iter()
            .map(|(&id, h)| (id, h.spatial_threat_values().to_vec()))
            .collect::<Vec<_>>(),
        maps
    );
}

#[test]
fn retained_grid_and_initialized_zero_both_affect_simulation_hash() {
    let (mut sim, rules, _) = fixture(7);
    let id = sim
        .construct_object_limbo_at_height("MTNK", "Americans", 13, 15, 0, 0, &rules)
        .unwrap();
    let initial = sim.state_hash();
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .retain_spatial_threat(0);
    let initialized = sim.state_hash();
    assert_ne!(
        initial, initialized,
        "Some0 changes the future required-read domain"
    );
    let russian = sim.interner.get("Russians").unwrap();
    sim.houses
        .get_mut(&russian)
        .unwrap()
        .adjust_spatial_threat((13, 15), 7)
        .unwrap();
    assert_ne!(
        sim.state_hash(),
        initialized,
        "House grid is gameplay authority"
    );
}

#[test]
fn foot_coefficient_is_copied_only_after_admitted_unlimbo() {
    use crate::util::native_x87::NativeF64Bits;
    let (mut sim, rules, _) = fixture(7);
    let id = sim
        .construct_object_limbo_at_height("MTNK", "Americans", 13, 15, 0, 0, &rules)
        .unwrap();
    assert_eq!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .navigation
            .path_threat_coefficient(),
        NativeF64Bits::POSITIVE_ZERO
    );
    // Retained test prestate uses a value in the saved native getter controls.
    let retained = NativeF64Bits::from_bits(0x3ff0000000000000);
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .navigation
        .retain_threat_avoidance_after_unlimbo(retained);
    for placement in [
        PlacementEvidence::RejectedEarly,
        PlacementEvidence::MarkFailed,
    ] {
        assert_eq!(
            sim.reveal_constructed_object_at_height(id, 13, 15, 0, 0, placement, &rules),
            None
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .navigation
                .path_threat_coefficient(),
            retained
        );
    }
    assert_eq!(
        sim.reveal_constructed_object_at_height(
            id,
            13,
            15,
            0,
            0,
            PlacementEvidence::EvaluateMark,
            &rules
        ),
        Some(id)
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .navigation
            .path_threat_coefficient(),
        rules.object("MTNK").unwrap().threat_avoidance_coefficient
    );
}

#[test]
fn real_garrison_append_and_complete_ejection_refresh_the_existing_building_cache() {
    let art = crate::rules::ini_parser::IniFile::from_str("[GARR]\nFoundation=1x1\n");
    let (mut sim, rules, registry) = fixture_with_additions(
        7,
        "[General]\nThreatPerOccupant=10\n[InfantryTypes]\n2=E1\n[BuildingTypes]\n1=GARR\n[E1]\nStrength=100\nSpeed=4\nSpeedType=Foot\nMovementZone=Normal\nOccupier=yes\nThreatPosed=7\nThreatAvoidanceCoefficient=0.5\nLocomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n[GARR]\nStrength=400\nCanBeOccupied=yes\nMaxNumberOccupants=5\nThreatPosed=7\n",
        &art,
    );
    let building = sim
        .spawn_object("GARR", "Americans", 16, 15, 0, &rules)
        .unwrap();
    let passenger = sim
        .spawn_object("E1", "Americans", 15, 15, 0, &rules)
        .unwrap();
    assert_eq!(
        sim.substrate
            .entities
            .get(building)
            .unwrap()
            .cached_spatial_threat(),
        Some(7)
    );
    sim.substrate
        .entities
        .get_mut(passenger)
        .unwrap()
        .passenger_role = crate::sim::passenger::PassengerRole::Boarding {
        target_transport_id: building,
    };
    crate::sim::passenger::tick_passenger_system(&mut sim, &rules, Some(&registry));
    assert_eq!(
        sim.substrate
            .entities
            .get(building)
            .unwrap()
            .passenger_role
            .cargo()
            .unwrap()
            .count(),
        1
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(passenger)
            .unwrap()
            .cached_spatial_threat(),
        Some(0),
        "passenger Limbo removes its retained contribution before append"
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(building)
            .unwrap()
            .cached_spatial_threat(),
        Some(rules.general.threat_per_occupant)
    );
    // Saved Foot+530 can differ from its type. The nested ejection Unlimbo
    // must copy the type anew; a barrel-only fallback cannot satisfy this.
    sim.substrate
        .entities
        .get_mut(passenger)
        .unwrap()
        .navigation
        .retain_threat_avoidance_after_unlimbo(crate::util::native_x87::NativeF64Bits::ONE);
    crate::sim::production::sell_building_occupants(&mut sim, &rules, Some(&registry), building);
    assert_eq!(
        sim.substrate
            .entities
            .get(passenger)
            .unwrap()
            .cached_spatial_threat(),
        Some(7),
        "nested admitted Unlimbo republishes passenger threat before Scatter"
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(passenger)
            .unwrap()
            .navigation
            .path_threat_coefficient(),
        rules.object("E1").unwrap().threat_avoidance_coefficient,
        "nested Foot Unlimbo executes the coefficient copy through the shared owner"
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(building)
            .unwrap()
            .passenger_role
            .cargo()
            .unwrap()
            .count(),
        0
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(building)
            .unwrap()
            .cached_spatial_threat(),
        Some(7),
        "ejection tail refreshes after the complete cargo mutation"
    );
}
