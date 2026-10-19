//! Native kernel/index execution plus production lifecycle integration.
//! Lifecycle fixtures supply accepted type and physical-position prestates;
//! they do not claim a full native scenario run. Arithmetic expectations come
//! from original56BC50/4FA2E0 execution, never from this Rust kernel.
use super::*;
use crate::sim::house_state::HouseState;
use crate::sim::world::PlacementEvidence;

fn packet() -> &'static serde_json::Value {
    crate::test_fixture::json("tools/spatial_oracle/astar_threat_inputs.json")
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
    crate::rules::overlay_types::OverlayTypeRegistry,
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
    crate::rules::overlay_types::OverlayTypeRegistry,
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
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    (sim, rules, registry)
}

fn actor(sim: &mut Simulation, rules: &RuleSet, cell: (u16, u16)) -> u64 {
    sim.spawn_object("MTNK", "Americans", cell.0, cell.1, 0, rules)
        .unwrap()
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
fn retained_grid_and_initialized_zero_both_affect_simulation_hash() {
    let (mut sim, rules, _) = fixture(7);
    let id = sim
        .construct_object_limbo_at_height(
            "MTNK",
            "Americans",
            13,
            15,
            0,
            0,
            &rules,
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
    let initial = sim.state_hash();
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .cached_spatial_threat
        .withdraw();
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
        .construct_object_limbo_at_height(
            "MTNK",
            "Americans",
            13,
            15,
            0,
            0,
            &rules,
            crate::sim::world::FrameEffects::default(),
        )
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
            sim.reveal_constructed_object_at_height(
                id,
                13,
                15,
                0,
                0,
                placement,
                &rules,
                crate::sim::world::FrameEffects::default()
            ),
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
            &rules,
            crate::sim::world::FrameEffects::default()
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
    assert!(sim.apply_command_with_overlays(
        "Americans",
        &crate::sim::command::Command::CaptureBuilding {
            engineer_id: passenger,
            target_building_id: building,
        },
        Some(&rules),
        Some(&registry),
        crate::sim::world::FrameEffects::default(),
    ));
    let grid = sim.path_grid_snapshot().unwrap();
    for _ in 0..64 {
        sim.advance_tick(&[], Some(&rules), Some(&grid), Some(&registry), 67);
        if sim
            .substrate
            .entities
            .get(passenger)
            .unwrap()
            .passenger_role
            .is_inside_transport()
        {
            break;
        }
    }
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
    crate::sim::passenger::garrison_eject::sell_building_occupants(
        &mut sim,
        &rules,
        Some(&registry),
        building,
        crate::sim::passenger::garrison_eject::GarrisonEjectMode::Release,
        crate::sim::world::FrameEffects::default(),
    );
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
