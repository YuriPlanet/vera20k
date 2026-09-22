//! End-to-end integration tests for the paradrop superweapon launch pipeline.
//!
//! Exercises: launch handler -> carrier spawn at waypoint edge -> limbo cargo
//! load -> Open-equivalent ParaDropApproach mission -> distance check ->
//! Rescue-equivalent ParaDropOverfly -> V-pattern Drop_Payload at the
//! Mission_Rescue cadence -> infantry calls begin_parachute_descent -> descent
//! ramp -> landing.

#![cfg(test)]

use std::collections::BTreeMap;

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::aircraft::AircraftMission;
use crate::sim::pathfinding::PathGrid;
use crate::sim::superweapon::paradrop::{ParaDropKind, launch};
use crate::sim::world::Simulation;
use crate::sim::world::edge_cell::{Edge, find_paradrop_edge_cell};

/// Minimal ruleset with PDPLANE + ParaDropWeapon + E1 + AMRADR cargo plane setup.
/// AmerParaDropNum trimmed to 4 for faster test cycles vs the vanilla 8.
fn make_paradrop_rules() -> RuleSet {
    make_paradrop_rules_with_flight_level(None)
}

fn make_paradrop_rules_with_flight_level(flight_level: Option<i32>) -> RuleSet {
    let text = "\
[InfantryTypes]
0=E1

[VehicleTypes]

[AircraftTypes]
0=PDPLANE

[BuildingTypes]

[General]
BuildSpeed=0.75
MultipleFactory=0.7
LowPowerPenaltyModifier=1.25
MinLowPowerProductionSpeed=0.4
MaxLowPowerProductionSpeed=0.85
ParadropRadius=1024
AmerParaDropInf=E1
AmerParaDropNum=4
AllyParaDropInf=E1
AllyParaDropNum=4
SovParaDropInf=E1
SovParaDropNum=4
YuriParaDropInf=E1
YuriParaDropNum=4
ParachuteMaxFallRate=-3
FlightLevel=1500

[E1]
Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}
Name=GI
Cost=200
Strength=125
Armor=none
Speed=4
Primary=M60

[PDPLANE]
Locomotor={4A582746-9839-11d1-B709-00A024DDAFD1}
Name=Cargo Plane
Strength=400
Armor=light
Speed=15
ROT=2
Primary=ParaDropWeapon
Spawned=yes
Selectable=no
Sight=0
Landable=no

[M60]
Damage=25
ROF=20
Range=5
Warhead=SA

[ParaDropWeapon]
Damage=60
ROF=130
Range=1
Warhead=SA

[SA]
Verses=100%,100%,100%,90%,70%,25%,100%,25%,25%,0%,0%
CellSpread=0
";
    let text = flight_level.map_or_else(
        || text.to_string(),
        |level| text.replace("[PDPLANE]", &format!("[PDPLANE]\nFlightLevel={level}")),
    );
    let ini = IniFile::from_str(&text);
    RuleSet::from_ini(&ini).expect("test ruleset parse")
}

fn make_paradrop_rules_with_limited_pdplane_cargo() -> RuleSet {
    let text = "\
[InfantryTypes]
0=E1

[VehicleTypes]

[AircraftTypes]
0=PDPLANE

[BuildingTypes]

[General]
ParadropRadius=1024
AmerParaDropInf=E1
AmerParaDropNum=4
ParachuteMaxFallRate=-3
FlightLevel=1500

[E1]
Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}
Name=GI
Strength=125
Armor=none
Speed=4
Size=2
Primary=M60

[PDPLANE]
Locomotor={4A582746-9839-11d1-B709-00A024DDAFD1}
Name=Cargo Plane
Strength=400
Armor=light
Speed=15
ROT=2
Primary=ParaDropWeapon
Spawned=yes
Selectable=no
Sight=0
Landable=no
Passengers=1
SizeLimit=1

[M60]
Damage=25
ROF=20
Range=5
Warhead=SA

[ParaDropWeapon]
Damage=60
ROF=130
Range=1
Warhead=SA

[SA]
Verses=100%,100%,100%,90%,70%,25%,100%,25%,25%,0%,0%
CellSpread=0
";
    let ini = IniFile::from_str(text);
    RuleSet::from_ini(&ini).expect("limited cargo paradrop ruleset parse")
}

/// Build a Simulation with a 100x100 fully-passable map and an "Americans"
/// house anchored at (50, 90) → waypoint_edge=North after transforming the
/// playfield's native LocalSize reference points into cell space.
fn install_paradrop_playfield(sim: &mut Simulation) {
    sim.fog.width = 200;
    sim.fog.height = 200;
    sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
        base: 100,
        off_fc: 0,
        off_100: 0,
        off_104: 100,
        off_108: 100,
    });
}

fn build_sim(rules: &RuleSet) -> (Simulation, PathGrid) {
    let mut sim = Simulation::new();
    install_paradrop_playfield(&mut sim);
    let owner_id = sim.interner.intern("Americans");
    let mut house = crate::sim::house_state::HouseState::new(
        owner_id, /*side_index*/ 0, /*country*/ None, /*is_human*/ true,
        /*credits*/ 10_000, /*tech_level*/ 10,
    );
    house.base_center = Some((50, 90));
    house.waypoint_edge = crate::sim::house_state::determine_waypoint_edge(
        (50, 90),
        sim.playfield_bounds.expect("test playfield bounds"),
    );
    sim.houses.insert(owner_id, house);
    let _ = rules;
    let path_grid = PathGrid::test_all_passable(200, 200);
    (sim, path_grid)
}

fn tick_n(sim: &mut Simulation, rules: &RuleSet, path_grid: &PathGrid, n: u32) {
    let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    for _ in 0..n {
        sim.advance_tick(&[], Some(rules), &height_map, Some(path_grid), None, 22);
    }
}

fn count_descending_infantry(sim: &Simulation) -> usize {
    sim.substrate
        .entities
        .values()
        .filter(|e| e.parachute_state.is_some())
        .count()
}

fn find_pdplane(sim: &Simulation) -> Option<u64> {
    sim.substrate
        .entities
        .values()
        .find(|e| {
            sim.interner
                .resolve(e.type_ref)
                .eq_ignore_ascii_case("PDPLANE")
                && e.health.current > 0
        })
        .map(|e| e.stable_id)
}

fn count_alive_infantry(sim: &Simulation, type_str: &str) -> usize {
    sim.substrate
        .entities
        .values()
        .filter(|e| {
            sim.interner
                .resolve(e.type_ref)
                .eq_ignore_ascii_case(type_str)
                && e.health.current > 0
        })
        .count()
}

#[test]
fn infantry_terminal_empty_custom_carrier_retires_after_failed_launch() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=PDPLANE\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
         [PDPLANE]\nStrength=400\nSpeed=15\nLocomotor={4A582746-9839-11D1-B709-00A024DDAFD1}\n\
         [General]\nAmerParaDropInf=PDPLANE\nAmerParaDropNum=0\nFlightLevel=1500\n",
    ))
    .unwrap();
    let (mut sim, path_grid) = build_sim(&rules);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner.intern("Americans");
    let sw = sim.interner.intern("SWTEST");
    assert!(!launch(
        &mut sim,
        &rules,
        owner,
        50,
        20,
        ParaDropKind::American,
        sw,
        Some(&path_grid)
    ));
    let carrier = sim
        .substrate
        .entities
        .values()
        .find(|entity| sim.interner.resolve(entity.type_ref()) == "PDPLANE")
        .unwrap();
    let id = carrier.stable_id();
    assert_eq!(
        carrier.category,
        crate::map::entities::EntityCategory::Infantry
    );
    assert!(carrier.dying && carrier.animation.is_some() && carrier.in_logic_vector);
    assert_eq!(
        carrier.infantry_terminal,
        Some(crate::sim::world::InfantryTerminal::RetireNextVisit)
    );
    tick_n(&mut sim, &rules, &path_grid, 1);
    assert!(!sim.substrate.entities.contains(id));
    assert!(!sim.live_object_order_snapshot().contains(&id));
}

#[test]
fn mission_only_paradrop_marks_even_an_ordinary_landable_type() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[AircraftTypes]\n0=PDPLANE\n[InfantryTypes]\n0=E1\n[VehicleTypes]\n[BuildingTypes]\n\
         [General]\nAmerParaDropInf=E1\nAmerParaDropNum=1\nFlightLevel=1500\n\
         [PDPLANE]\nStrength=100\nSpeed=10\nSelectable=yes\nLandable=yes\n\
         Locomotor={4A582746-9839-11D1-B709-00A024DDAFD1}\n[E1]\nStrength=100\n",
    ))
    .unwrap();
    let (mut sim, path_grid) = build_sim(&rules);
    let owner = sim.interner.intern("Americans");
    let sw = sim.interner.intern("SWTEST");
    assert!(launch(
        &mut sim,
        &rules,
        owner,
        50,
        20,
        ParaDropKind::American,
        sw,
        Some(&path_grid)
    ));
    let id = find_pdplane(&sim).unwrap();
    assert!(sim.substrate.entities.get(id).unwrap().is_mission_only());
    assert!(
        sim.substrate
            .entities
            .values()
            .filter(|e| e.stable_id() != id)
            .all(|e| !e.is_mission_only()),
        "payload constructors do not inherit carrier history"
    );
}

#[test]
fn paradrop_launch_spawns_carrier_with_loaded_cargo() {
    let rules = make_paradrop_rules();
    let (mut sim, path_grid) = build_sim(&rules);
    let owner = sim.interner.intern("Americans");

    // Target near the north edge of the map.
    let sw_test = sim.interner.intern("SWTEST");
    let ok = launch(
        &mut sim,
        &rules,
        owner,
        50,
        20,
        ParaDropKind::American,
        sw_test,
        Some(&path_grid),
    );
    assert!(ok, "launch should succeed with valid waypoint edge + cargo");

    // Exactly one PDPLANE — AmerParaDropInf is single-entry (E1).
    let pdplane_id = find_pdplane(&sim).expect("PDPLANE must exist");

    // Cargo: 4 E1 (AmerParaDropNum=4 in test rules).
    let cargo_count = sim
        .substrate
        .entities
        .get(pdplane_id)
        .unwrap()
        .passenger_role
        .cargo()
        .map(|c| c.count())
        .unwrap_or(0);
    assert_eq!(cargo_count, 4, "cargo should be loaded with 4 E1");

    // Mission set to the Open-equivalent paradrop state with target + fog latch cleared.
    match sim
        .substrate
        .entities
        .get(pdplane_id)
        .unwrap()
        .aircraft_mission
    {
        Some(AircraftMission::ParaDropApproach {
            target_rx,
            target_ry,
            has_revealed_fog,
        }) => {
            assert_eq!(target_rx, 50);
            assert_eq!(target_ry, 20);
            assert!(!has_revealed_fog);
        }
        ref other => panic!("expected Open-equivalent ParaDropApproach, got {:?}", other),
    }
}

#[test]
fn paradrop_type_flight_level_reaches_spawned_carrier_and_survives_restore() {
    for configured in [-1, 2200] {
        let rules = make_paradrop_rules_with_flight_level(Some(configured));
        let (mut sim, path_grid) = build_sim(&rules);
        let owner = sim.interner.intern("Americans");
        let sw_test = sim.interner.intern("SWTEST");
        assert!(launch(
            &mut sim,
            &rules,
            owner,
            50,
            20,
            ParaDropKind::American,
            sw_test,
            Some(&path_grid),
        ));
        let id = find_pdplane(&sim).unwrap();
        let expected = if configured == -1 { 1500 } else { configured };
        let restored: Simulation =
            bincode::deserialize(&bincode::serialize(&sim).unwrap()).unwrap();
        for world in [&sim, &restored] {
            let plane = world.substrate.entities.get(id).unwrap();
            let loco = plane.locomotor.as_ref().unwrap();
            assert_eq!(loco.fly_target_height(), expected);
            assert_eq!(loco.altitude.to_num::<i32>(), expected);
            assert_eq!(world.foot_navigation_coordinate(id).unwrap().z, expected);
            assert_eq!(plane.passenger_role.cargo().unwrap().count(), 4);
        }
    }
}

#[test]
fn paradrop_multi_entry_resolves_each_edge_after_its_carrier_constructor() {
    let mut rules = make_paradrop_rules();
    rules.general.amer_paradrop_list = vec![("E1".to_string(), 1), ("E1".to_string(), 1)];
    let (mut sim, path_grid) = build_sim(&rules);
    sim.scenario_rng = crate::sim::rng::SimRng::new(0x65E6_6000);
    let owner = sim.interner.intern("Americans");
    let bounds = sim.playfield_bounds.expect("playfield authority");

    let mut expected_rng = sim.scenario_rng.clone();
    let mut expected_cells = Vec::new();
    for _ in 0..2 {
        let _carrier_constructor_word = expected_rng.next_u32();
        expected_cells.push(
            find_paradrop_edge_cell(Some(bounds), None, Edge::North, &mut expected_rng)
                .expect("North edge cell"),
        );
        let _passenger_constructor_word = expected_rng.next_u32();
    }

    let sw_test = sim.interner.intern("SWTEST");
    assert!(launch(
        &mut sim,
        &rules,
        owner,
        50,
        20,
        ParaDropKind::American,
        sw_test,
        Some(&path_grid),
    ));
    let mut actual_cells = sim
        .substrate
        .entities
        .values()
        .filter(|entity| {
            sim.interner
                .resolve(entity.type_ref)
                .eq_ignore_ascii_case("PDPLANE")
        })
        .map(|entity| (entity.stable_id, (entity.position.rx, entity.position.ry)))
        .collect::<Vec<_>>();
    actual_cells.sort_by_key(|entry| entry.0);
    assert_eq!(
        actual_cells.iter().map(|entry| entry.1).collect::<Vec<_>>(),
        expected_cells,
    );
    assert_eq!(
        sim.scenario_rng.logical_state(),
        expected_rng.logical_state()
    );
}

#[test]
fn paradrop_launch_ignores_blocked_ground_spawn_edge() {
    let rules = make_paradrop_rules();
    let (mut sim, _path_grid) = build_sim(&rules);
    let owner = sim.interner.intern("Americans");
    let blocked_grid = PathGrid::test_all_blocked(200, 200);

    let sw_test = sim.interner.intern("SWTEST");
    let ok = launch(
        &mut sim,
        &rules,
        owner,
        50,
        20,
        ParaDropKind::American,
        sw_test,
        Some(&blocked_grid),
    );

    assert!(ok, "carrier edge helper should bypass ground passability");
    let pdplane_id = find_pdplane(&sim).expect("PDPLANE must exist");
    let pdplane = sim.substrate.entities.get(pdplane_id).unwrap();
    let bounds = sim.playfield_bounds.expect("playfield authority");
    assert!(
        !bounds.contains_height_aware_packed(
            i32::from(pdplane.position.rx),
            i32::from(pdplane.position.ry),
            0,
            0,
        ),
        "criterion-4 carrier spawn remains outside the playfield even when ground cells are blocked"
    );
}

#[test]
fn paradrop_launch_invalid_waypoint_edge_falls_back_to_north() {
    let rules = make_paradrop_rules();
    let (mut sim, path_grid) = build_sim(&rules);
    let owner = sim.interner.intern("Americans");
    sim.houses.get_mut(&owner).unwrap().waypoint_edge = 255;

    let sw_test = sim.interner.intern("SWTEST");
    let ok = launch(
        &mut sim,
        &rules,
        owner,
        40,
        20,
        ParaDropKind::American,
        sw_test,
        Some(&path_grid),
    );

    assert!(ok, "invalid waypoint edge should not abort standard launch");
    let pdplane_id = find_pdplane(&sim).expect("PDPLANE must exist");
    let pdplane = sim.substrate.entities.get(pdplane_id).unwrap();
    let bounds = sim.playfield_bounds.expect("playfield authority");
    assert!(
        !bounds.contains_height_aware_packed(
            i32::from(pdplane.position.rx),
            i32::from(pdplane.position.ry),
            0,
            0,
        ),
        "invalid edge fallback still uses the native outside North perimeter"
    );
}

#[test]
fn paradrop_cargo_load_bypasses_pdplane_capacity_and_passenger_occupancy() {
    let rules = make_paradrop_rules_with_limited_pdplane_cargo();
    let (mut sim, path_grid) = build_sim(&rules);
    let owner = sim.interner.intern("Americans");

    let sw_test = sim.interner.intern("SWTEST");
    let ok = launch(
        &mut sim,
        &rules,
        owner,
        50,
        20,
        ParaDropKind::American,
        sw_test,
        Some(&path_grid),
    );

    assert!(ok, "launch should load the configured paradrop payload");
    let pdplane_id = find_pdplane(&sim).expect("PDPLANE must exist");
    let pdplane = sim.substrate.entities.get(pdplane_id).unwrap();
    let edge_cell = (pdplane.position.rx, pdplane.position.ry);
    let cargo = pdplane
        .passenger_role
        .cargo()
        .expect("PDPLANE should have cargo");

    assert_eq!(
        cargo.capacity, 1,
        "test PDPLANE has a normal cargo cap of 1"
    );
    assert_eq!(cargo.size_limit, 1, "test PDPLANE rejects Size=2 normally");
    assert_eq!(cargo.count(), 4, "AmerParaDropNum should drive cargo count");
    assert_eq!(
        cargo.total_size, 8,
        "forced loading should still track Size="
    );

    for &pax_id in &cargo.passengers {
        assert!(
            !sim.substrate
                .occupancy
                .contains_entity(edge_cell.0, edge_cell.1, pax_id),
            "limbo-loaded passenger should not transiently occupy the spawn edge"
        );
    }
}

#[test]
fn paradrop_full_pipeline_drops_infantry_until_cargo_empty() {
    let rules = make_paradrop_rules();
    let (mut sim, path_grid) = build_sim(&rules);
    let owner = sim.interner.intern("Americans");

    let pre_e1_count = count_alive_infantry(&sim, "E1");
    assert_eq!(pre_e1_count, 0, "no E1 should exist pre-launch");

    let sw_test = sim.interner.intern("SWTEST");
    let ok = launch(
        &mut sim,
        &rules,
        owner,
        50,
        20,
        ParaDropKind::American,
        sw_test,
        Some(&path_grid),
    );
    assert!(ok, "launch should succeed");

    let pdplane_id = find_pdplane(&sim).expect("PDPLANE must exist");

    // Right after launch, 4 E1 exist as Inside-cargo (hidden).
    assert_eq!(count_alive_infantry(&sim, "E1"), 4, "4 E1 should be loaded");
    assert_eq!(count_descending_infantry(&sim), 0, "none descending yet");

    // Tick until cargo empties at the 5-game-frame Mission_Rescue cadence,
    // with headroom for the approach flight from south to within ParadropRadius.
    let mut first_drop_tick: Option<u32> = None;
    for tick in 0..2000u32 {
        let cargo_before = sim
            .substrate
            .entities
            .get(pdplane_id)
            .and_then(|e| e.passenger_role.cargo())
            .map(|c| c.count())
            .unwrap_or(0);
        tick_n(&mut sim, &rules, &path_grid, 1);
        let cargo_after = sim
            .substrate
            .entities
            .get(pdplane_id)
            .and_then(|e| e.passenger_role.cargo())
            .map(|c| c.count())
            .unwrap_or(0);
        if cargo_after < cargo_before && first_drop_tick.is_none() {
            first_drop_tick = Some(tick);
        }
        if cargo_after == 0 {
            break;
        }
    }

    assert!(
        first_drop_tick.is_some(),
        "first drop should fire within 2000 ticks"
    );

    // After all drops, 4 infantry exist as descending (parachute_state Some).
    let descending = count_descending_infantry(&sim);
    assert!(
        descending >= 1,
        "at least one infantry should be descending after drops; got {}",
        descending,
    );
}

#[test]
fn paradrop_descent_ends_with_landed_infantry_and_carrier_despawned() {
    let rules = make_paradrop_rules();
    let (mut sim, path_grid) = build_sim(&rules);
    let owner = sim.interner.intern("Americans");

    let sw_test = sim.interner.intern("SWTEST");
    launch(
        &mut sim,
        &rules,
        owner,
        50,
        20,
        ParaDropKind::American,
        sw_test,
        Some(&path_grid),
    );

    // Run for plenty of ticks to drain cargo + descent + carrier exit.
    // ~520 ticks for drops + ~500 ticks for max descent + ~1000 for exit flight.
    tick_n(&mut sim, &rules, &path_grid, 4000);

    // No descending infantry remain.
    assert_eq!(
        count_descending_infantry(&sim),
        0,
        "all infantry should have landed",
    );

    // Some E1 alive on the ground (parachute_state cleared).
    let landed = sim
        .substrate
        .entities
        .values()
        .filter(|e| {
            sim.interner.resolve(e.type_ref).eq_ignore_ascii_case("E1")
                && e.health.current > 0
                && e.parachute_state.is_none()
        })
        .count();
    assert!(
        landed >= 1,
        "at least one E1 should have landed alive (got {})",
        landed
    );

    // Carrier despawned (silent_despawn at boundary).
    assert!(
        find_pdplane(&sim).is_none(),
        "PDPLANE should have despawned at the exit boundary",
    );
}

#[test]
fn paradrop_per_side_branch_picks_correct_list() {
    // Generic ParaDrop with side_index=2 should use the Yuri list.
    // We verify this by setting different counts per side and checking which
    // count gets loaded into the carrier.
    let text = "\
[InfantryTypes]
0=E1
1=E2
2=INIT

[VehicleTypes]
[AircraftTypes]
0=PDPLANE
[BuildingTypes]

[General]
ParadropRadius=1024
AmerParaDropInf=E1
AmerParaDropNum=2
AllyParaDropInf=E1
AllyParaDropNum=3
SovParaDropInf=E2
SovParaDropNum=4
YuriParaDropInf=INIT
YuriParaDropNum=5
ParachuteMaxFallRate=-3

[E1]
Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}
Name=GI
Strength=125
Armor=none
Speed=4
Primary=M60

[E2]
Name=Conscript
Strength=100
Armor=none
Speed=4
Primary=M60

[INIT]
Name=Initiate
Strength=100
Armor=none
Speed=4
Primary=M60

[PDPLANE]
Locomotor={4A582746-9839-11d1-B709-00A024DDAFD1}
Strength=400
Armor=light
Speed=15
ROT=2
Primary=ParaDropWeapon
Spawned=yes
Sight=0
Landable=no

[M60]
Damage=25
ROF=20
Range=5
Warhead=SA

[ParaDropWeapon]
Damage=60
ROF=130
Range=1
Warhead=SA

[SA]
Verses=100%,100%,100%,90%,70%,25%,100%,25%,25%,0%,0%
CellSpread=0
";
    let rules = RuleSet::from_ini(&IniFile::from_str(text)).expect("rules parse");

    // Yuri side (index=2) → loads INIT × 5.
    {
        let mut sim = Simulation::new();
        install_paradrop_playfield(&mut sim);
        let owner_id = sim.interner.intern("Yuri");
        let mut house =
            crate::sim::house_state::HouseState::new(owner_id, 2, None, true, 10_000, 10);
        house.waypoint_edge = 2; // South
        sim.houses.insert(owner_id, house);
        let path_grid = PathGrid::test_all_passable(200, 200);

        let sw_test = sim.interner.intern("SWTEST");
        launch(
            &mut sim,
            &rules,
            owner_id,
            50,
            20,
            ParaDropKind::Generic,
            sw_test,
            Some(&path_grid),
        );

        let pdplane_id = find_pdplane(&sim).expect("PDPLANE must exist");
        let cargo_count = sim
            .substrate
            .entities
            .get(pdplane_id)
            .unwrap()
            .passenger_role
            .cargo()
            .map(|c| c.count())
            .unwrap_or(0);
        assert_eq!(cargo_count, 5, "Yuri side should load 5 INIT");

        // Verify cargo type is INIT.
        let cargo_ids = sim
            .substrate
            .entities
            .get(pdplane_id)
            .unwrap()
            .passenger_role
            .cargo()
            .unwrap()
            .passengers
            .clone();
        for pax_id in cargo_ids {
            let type_str = sim
                .interner
                .resolve(sim.substrate.entities.get(pax_id).unwrap().type_ref);
            assert!(
                type_str.eq_ignore_ascii_case("INIT"),
                "Yuri cargo should be INIT, got {}",
                type_str,
            );
        }
    }

    // Soviet side (index=1, falls through default arm) → loads E2 × 4.
    {
        let mut sim = Simulation::new();
        install_paradrop_playfield(&mut sim);
        let owner_id = sim.interner.intern("Russians");
        let mut house =
            crate::sim::house_state::HouseState::new(owner_id, 1, None, true, 10_000, 10);
        house.waypoint_edge = 2;
        sim.houses.insert(owner_id, house);
        let path_grid = PathGrid::test_all_passable(200, 200);

        let sw_test = sim.interner.intern("SWTEST");
        launch(
            &mut sim,
            &rules,
            owner_id,
            50,
            20,
            ParaDropKind::Generic,
            sw_test,
            Some(&path_grid),
        );

        let pdplane_id = find_pdplane(&sim).expect("PDPLANE must exist");
        let cargo_count = sim
            .substrate
            .entities
            .get(pdplane_id)
            .unwrap()
            .passenger_role
            .cargo()
            .map(|c| c.count())
            .unwrap_or(0);
        assert_eq!(cargo_count, 4, "Soviet side should load 4 E2");
    }

    // Unknown side (index=99, also falls through to Soviet branch) → 4 E2.
    {
        let mut sim = Simulation::new();
        install_paradrop_playfield(&mut sim);
        let owner_id = sim.interner.intern("Unknown");
        let mut house =
            crate::sim::house_state::HouseState::new(owner_id, 99, None, true, 10_000, 10);
        house.waypoint_edge = 2;
        sim.houses.insert(owner_id, house);
        let path_grid = PathGrid::test_all_passable(200, 200);

        let sw_test = sim.interner.intern("SWTEST");
        launch(
            &mut sim,
            &rules,
            owner_id,
            50,
            20,
            ParaDropKind::Generic,
            sw_test,
            Some(&path_grid),
        );

        let pdplane_id = find_pdplane(&sim).expect("PDPLANE must exist");
        let cargo_count = sim
            .substrate
            .entities
            .get(pdplane_id)
            .unwrap()
            .passenger_role
            .cargo()
            .map(|c| c.count())
            .unwrap_or(0);
        assert_eq!(
            cargo_count, 4,
            "Unknown side should fall back to Soviet (4 E2)"
        );
    }
}
