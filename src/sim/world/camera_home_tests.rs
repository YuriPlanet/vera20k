use super::*;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::production::ProductionCategory;

fn fixture() -> (Simulation, RuleSet, OverlayTypeRegistry, [InternedId; 2]) {
    let (mut sim, rules, registry) = crate::sim::world::entry_test_fixture::fixture_with_rules(
        "[Countries]\n0=LocalCountry\n1=OtherCountry\n[LocalCountry]\n[OtherCountry]\n\
         [General]\nBaseUnit=BASEUNIT\n\
         [VehicleTypes]\n0=BASEUNIT\n1=OTHERUNIT\n\
         [BASEUNIT]\nStrength=300\nSpeed=5\nSpeedType=Track\nMovementZone=Normal\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
         [OTHERUNIT]\nStrength=300\nSpeed=5\nSpeedType=Track\nMovementZone=Normal\nDeploysInto=YARD\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
         [BuildingTypes]\n0=YARD\n1=OTHER\n\
         [YARD]\nStrength=500\nCost=1000\nFoundation=1x1\nConstructionYard=yes\nFactory=BuildingType\n\
         [OTHER]\nStrength=500\nCost=1000\nFoundation=1x1\nUndeploysInto=BASEUNIT\n",
    );
    let houses = std::array::from_fn(|i| {
        let name = sim.interner.intern(if i == 0 { "Local" } else { "Other" });
        let country = sim.interner.intern(if i == 0 {
            "LocalCountry"
        } else {
            "OtherCountry"
        });
        sim.houses.register(HouseState::new(
            name,
            i as u8,
            Some(country),
            i == 0,
            10000,
            10,
        ));
        name
    });
    sim.session.set_current_house_for_test(Some(houses[0]));
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    (sim, rules, registry, houses)
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, kind: &str, owner: &str, x: u16, y: u16) -> u64 {
    sim.spawn_object(kind, owner, x, y, 0, rules).unwrap()
}

fn clear_base(sim: &mut Simulation, owner: InternedId) {
    let house = sim.houses.get_mut(&owner).unwrap();
    house.set_base_geometry_for_test(None, 0);
    house.set_alternate_base_center_for_test((0, 0));
}

fn query(sim: &Simulation, rules: &RuleSet) -> Option<[i32; 3]> {
    let cells = sim
        .resolved_terrain
        .as_ref()
        .map(NativeCellQuery::canonical);
    sim.home_camera_coordinate(rules, cells.as_ref())
}

fn coord(sim: &Simulation, id: u64) -> [i32; 3] {
    let point = object_get_coords(
        sim.substrate.entities.get(id).unwrap(),
        sim.resolved_terrain.as_ref(),
    );
    [point.x, point.y, point.z]
}

#[test]
fn home_uses_native_reverse_yard_primary_limbo_and_other_building_priorities() {
    // Original7315A0 controls: earliest_yard, primary_newer/older,
    // skip_limbo_primary, yard_ignores_health_alive and last_other.
    // Steam gamemd. The original observes GetCoords ABI inputs;
    // these checks use actual production objects and their shared coordinate owner.
    let (mut sim, rules, _, houses) = fixture();
    let first = spawn(&mut sim, &rules, "YARD", "Local", 15, 15);
    let other = spawn(&mut sim, &rules, "OTHER", "Local", 17, 15);
    let second = spawn(&mut sim, &rules, "YARD", "Local", 19, 15);
    clear_base(&mut sim, houses[0]);
    sim.production.clear_primary_factory(first);
    sim.production.clear_primary_factory(second);
    let rng = sim.rng_state();
    assert_eq!(
        query(&sim, &rules),
        Some(coord(&sim, first)),
        "no primary: earliest yard"
    );
    sim.production
        .set_primary_factory_for_test(houses[0], ProductionCategory::Building, second);
    assert_eq!(query(&sim, &rules), Some(coord(&sim, second)));
    sim.production
        .set_primary_factory_for_test(houses[0], ProductionCategory::Building, first);
    assert_eq!(query(&sim, &rules), Some(coord(&sim, first)));
    sim.substrate
        .entities
        .get_mut(first)
        .unwrap()
        .lifecycle
        .set_in_limbo_for_test(true);
    assert_eq!(query(&sim, &rules), Some(coord(&sim, second)));
    {
        let first = sim.substrate.entities.get_mut(first).unwrap();
        first.lifecycle.set_in_limbo_for_test(false);
        first.lifecycle.set_object_alive_for_test(false);
        first.health.set_for_test(0);
    }
    assert_eq!(
        query(&sim, &rules),
        Some(coord(&sim, first)),
        "health/alive do not add admission gates"
    );
    sim.substrate
        .entities
        .get_mut(first)
        .unwrap()
        .lifecycle
        .set_in_limbo_for_test(true);
    sim.substrate
        .entities
        .get_mut(second)
        .unwrap()
        .lifecycle
        .set_in_limbo_for_test(true);
    assert_eq!(
        query(&sim, &rules),
        Some(coord(&sim, other)),
        "UndeploysInto does not make a ConstructionYard"
    );
    assert_eq!(sim.rng_state(), rng);
    let last_other = spawn(&mut sim, &rules, "OTHER", "Local", 21, 15);
    let rng = sim.rng_state();
    clear_base(&mut sim, houses[0]);
    assert_eq!(query(&sim, &rules), Some(coord(&sim, last_other)));
    assert_eq!(sim.rng_state(), rng);
}

#[test]
fn home_base_units_use_retained_count_rule_membership_and_existing_human_player_query() {
    // Original last_base_unit, skip_limbo_unit, campaign_control_house,
    // current_house_unit_count_gate and other_building_blocks_unit_scan.
    let (mut sim, rules, _, houses) = fixture();
    let first = spawn(&mut sim, &rules, "BASEUNIT", "Local", 15, 17);
    let other = spawn(&mut sim, &rules, "BASEUNIT", "Other", 17, 17);
    let misleading = spawn(&mut sim, &rules, "OTHERUNIT", "Local", 19, 17);
    let last = spawn(&mut sim, &rules, "BASEUNIT", "Local", 21, 17);
    clear_base(&mut sim, houses[0]);
    assert!(sim.houses[&houses[0]].tracking().vehicles() != 0);
    assert_eq!(query(&sim, &rules), Some(coord(&sim, last)));
    sim.substrate
        .entities
        .get_mut(last)
        .unwrap()
        .lifecycle
        .set_in_limbo_for_test(true);
    assert_eq!(
        query(&sim, &rules),
        Some(coord(&sim, first)),
        "DeploysInto is not BaseUnit membership"
    );
    assert_ne!(coord(&sim, misleading), coord(&sim, first));
    sim.houses
        .get_mut(&houses[1])
        .unwrap()
        .set_player_control_for_test(true);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(false));
    assert_eq!(query(&sim, &rules), Some(coord(&sim, other)));
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    assert_eq!(query(&sim, &rules), Some(coord(&sim, first)));
    let building = spawn(&mut sim, &rules, "OTHER", "Local", 16, 15);
    clear_base(&mut sim, houses[0]);
    assert_eq!(
        query(&sim, &rules),
        Some(coord(&sim, building)),
        "an ordinary building suppresses the MCV scan"
    );

    let (mut empty_owner, rules, _, houses) = fixture();
    spawn(&mut empty_owner, &rules, "BASEUNIT", "Other", 16, 17);
    empty_owner
        .session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(false));
    empty_owner
        .houses
        .get_mut(&houses[1])
        .unwrap()
        .set_player_control_for_test(true);
    assert_eq!(empty_owner.houses[&houses[0]].tracking().vehicles(), 0);
    assert_eq!(query(&empty_owner, &rules), None);
}

#[test]
fn home_base_fallback_is_shared_and_local_input_cannot_write_simulation_dummy() {
    let (mut sim, rules, _, houses) = fixture();
    let other = spawn(&mut sim, &rules, "OTHER", "Local", 15, 15);
    let origin = (18, 19);
    sim.houses
        .get_mut(&houses[0])
        .unwrap()
        .set_base_center(origin);
    let cells = sim
        .resolved_terrain
        .as_ref()
        .map(NativeCellQuery::canonical);
    let base = sim.house_base_coordinate(houses[0], cells.as_ref());
    assert_eq!(
        query(&sim, &rules),
        Some(base),
        "50DF30 overrides the other-Building fallback"
    );
    assert_ne!(base, coord(&sim, other));
    sim.houses
        .get_mut(&houses[0])
        .unwrap()
        .set_alternate_base_center_for_test((19, 20));
    let cells = sim
        .resolved_terrain
        .as_ref()
        .map(NativeCellQuery::canonical);
    let alternate = sim.house_base_coordinate(houses[0], cells.as_ref());
    assert_ne!(alternate, base);
    assert_eq!(query(&sim, &rules), Some(alternate));
    let yard = spawn(&mut sim, &rules, "YARD", "Local", 17, 15);
    sim.houses
        .get_mut(&houses[0])
        .unwrap()
        .set_alternate_base_center_for_test((500, 500));
    let dummy = sim.effective_shared_cell_dummy();
    dummy.stamp_coord(7, 9);
    let before = sim.state_hash();
    let rng = sim.rng_state();
    let isolated = NativeCellQuery::isolated(sim.resolved_terrain.as_ref().unwrap());
    assert_eq!(
        sim.home_camera_coordinate(&rules, Some(&isolated)),
        Some(coord(&sim, yard))
    );
    assert_eq!(
        sim.state_hash(),
        before,
        "H's isolated query does not mutate peer state"
    );
    assert_eq!(dummy.snapshot().coord, (7, 9));
    assert_eq!(query(&sim, &rules), Some(coord(&sim, yard)));
    assert_eq!(
        dummy.snapshot().coord,
        (500, 500),
        "the base query runs even when the yard wins"
    );
    assert_eq!(
        sim.state_hash(),
        before,
        "unretained Dummy coordinates follow the existing hash policy"
    );
    dummy.stamp_coord(7, 9);
    sim.house_outside_zones(houses[0], [100, 100, 0]);
    assert_eq!(
        dummy.snapshot().coord,
        (500, 500),
        "WhichZone uses the same corrected coordinate owner"
    );
    assert_eq!(sim.rng_state(), rng);
}

#[test]
fn home_primary_and_base_geometry_survive_real_snapshot_restore() {
    let (mut sim, rules, _, houses) = fixture();
    let first = spawn(&mut sim, &rules, "YARD", "Local", 15, 15);
    let second = spawn(&mut sim, &rules, "YARD", "Local", 19, 15);
    sim.production
        .set_primary_factory_for_test(houses[0], ProductionCategory::Building, second);
    sim.houses
        .get_mut(&houses[0])
        .unwrap()
        .set_alternate_base_center_for_test((18, 19));
    let expected = query(&sim, &rules);
    assert_eq!(expected, Some(coord(&sim, second)));
    assert_ne!(expected, Some(coord(&sim, first)));
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "Home camera", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    restored.restore_after_snapshot_load().unwrap();
    restored.resolve_type_handles(&rules);
    assert_eq!(query(&restored, &rules), expected);
    restored.session.set_current_house_for_test(None);
    assert_eq!(query(&restored, &rules), None, "headless boundary");
}
