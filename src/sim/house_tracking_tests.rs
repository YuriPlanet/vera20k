//! Tests for the house counts. `native_tracking_corpus` compares against
//! `tools/spatial_oracle/house_tracking.json`, produced by running the
//! original Add_Tracking and Remove_Tracking under Unicorn.

use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;

const RULES: &str = "\
[InfantryTypes]
0=GI
1=GID
[VehicleTypes]
0=TANK
1=TANKI
2=TANKD
3=BOAT
4=TRANSPORT
5=HARV
[AircraftTypes]
0=PLANE
1=PLANED
[BuildingTypes]
0=BLDG
1=BLDGI
2=BLDGD
3=B1X1
4=BGAT
5=BOTHER
6=PAD
[GI]
Strength=100
[GID]
Strength=100
DontScore=yes
[TANK]
Strength=300
[TANKI]
Strength=300
Insignificant=yes
[TANKD]
Strength=300
DontScore=yes
[BOAT]
Strength=300
Naval=yes
[TRANSPORT]
Strength=300
Naval=yes
Passengers=3
[HARV]
Strength=300
ResourceGatherer=yes
[PLANE]
Strength=200
[PLANED]
Strength=200
DontScore=yes
[BLDG]
Strength=500
[BLDGI]
Strength=500
Insignificant=yes
[BLDGD]
Strength=500
DontScore=yes
[B1X1]
Strength=500
UndeploysInto=TANK
[BGAT]
Strength=500
UndeploysInto=HARV
[BOTHER]
Strength=500
UndeploysInto=TANK
[PAD]
Strength=500
Helipad=yes
NumberOfDocks=4
";

const ART: &str = "[BLDG]\nFoundation=2x2\n[BLDGI]\nFoundation=2x2\n\
    [BLDGD]\nFoundation=2x2\n[B1X1]\nFoundation=1x1\n\
    [BGAT]\nFoundation=2x2\n[BOTHER]\nFoundation=2x2\n[PAD]\nFoundation=2x2\n";

#[derive(serde::Deserialize)]
struct NativeTrackingCase {
    input: serde_json::Value,
    events: Vec<serde_json::Value>,
    counts: std::collections::BTreeMap<String, i32>,
}

/// The fixture type standing for each oracle case, or `None` where VERA has
/// no such class (the oracle's non-Techno RTTI).
fn fixture_type(case: &str) -> Option<&'static str> {
    Some(match case.split_once('_').map_or(case, |(_, kind)| kind) {
        "unit" => "TANK",
        "unit_insignificant" => "TANKI",
        "unit_dont_score" => "TANKD",
        "naval_no_passengers" => "BOAT",
        "naval_transport" => "TRANSPORT",
        "aircraft" => "PLANE",
        "aircraft_dont_score" => "PLANED",
        "building" => "BLDG",
        "building_insignificant" => "BLDGI",
        "building_dont_score" => "BLDGD",
        "building_1x1_undeployer" => "B1X1",
        "building_undeploys_to_gatherer" => "BGAT",
        "building_undeploys_to_other" => "BOTHER",
        "infantry" | "infantry_latched" | "infantry_byte_439" => "GI",
        "infantry_dont_score" => "GID",
        _ => return None,
    })
}

/// `HouseClass::Add_Tracking @ 0x004FF700` and `Remove_Tracking @
/// 0x004FF550` against the original, through VERA's own writers: each case's
/// type is constructed (Add_Tracking), then UnInit'd and drained (the
/// destructor's Remove_Tracking), and the change in the two counters the
/// defeat gate reads is compared with the native routing: `+0x2F0` (a
/// building unless Insignificant, DontScore, a 1x1 undeployer or one that
/// undeploys into a ResourceGatherer) and the per-type `+0x5514` (a Unit
/// unless Insignificant or DontScore). The other counters the functions
/// write (`+0x2E8`, `+0x2EC`, `+0x2F4` and its latch, `+0x2F8`) are not kept.
#[test]
fn native_tracking_corpus() {
    let cases: Vec<NativeTrackingCase> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/house_tracking.json",
    ))
    .unwrap();
    assert_eq!(cases.len(), 36);
    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str(RULES),
        &IniFile::from_str(ART),
    )
    .unwrap();
    let mut compared = 0;
    for case in cases {
        let name = case.input["name"].as_str().unwrap();
        let native_units: i32 = case
            .events
            .iter()
            .filter(|event| event[1] == "units")
            .map(|event| if event[0] == "increment" { 1 } else { -1 })
            .sum();
        let native_buildings = case.counts["buildings"];
        let Some(kind) = fixture_type(name) else {
            assert_eq!((native_units, native_buildings), (0, 0), "{name}");
            continue;
        };
        let mut sim = Simulation::with_seed(3);
        let house = sim.interner.intern("Americans");
        sim.houses
            .insert(house, HouseState::new(house, 0, None, true, 0, 10));
        let counts = |sim: &Simulation| {
            let tracking = &sim.houses[&house].tracking;
            (tracking.buildings(), tracking.units_for_test())
        };
        let before = counts(&sim);
        let id = sim
            .spawn_object_at_height(kind, "Americans", 10, 10, 0, 0, &rules)
            .unwrap_or_else(|| panic!("{name}: {kind} spawns"));
        let added = counts(&sim);
        let delta = if case.input["call"] == "add" {
            (added.0 - before.0, added.1 - before.1)
        } else {
            sim.uninit_with_rules(id, &rules);
            sim.flush_pending_delete();
            let removed = counts(&sim);
            (removed.0 - added.0, removed.1 - added.1)
        };
        assert_eq!(delta, (native_buildings, native_units), "{name}");
        compared += 1;
    }
    assert_eq!(compared, 34);
}

fn one_house() -> (Simulation, RuleSet, InternedId) {
    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str(RULES),
        &IniFile::from_str(ART),
    )
    .unwrap();
    let mut sim = Simulation::with_seed(3);
    let house = sim.interner.intern("Americans");
    sim.houses
        .insert(house, HouseState::new(house, 0, None, true, 0, 10));
    (sim, rules, house)
}

/// Added_To_Game at Unlimbo (`0x006F6D8F`) and Removed_From_Game at the first
/// Limbo (`0x006F6BD1`) move the on-map counts the normal game reads.
/// `DontScore=` skips them, except that a Unit is added without the test
/// (`0x00502CF9`) and removed with it, so it stays counted.
#[test]
fn on_map_counts_follow_unlimbo_and_limbo() {
    let (mut sim, rules, house) = one_house();
    let kinds = [
        "TANK", "GI", "PLANE", "BLDG", "TANKD", "GID", "PLANED", "BLDGD",
    ];
    let ids: Vec<u64> = kinds
        .iter()
        .enumerate()
        .map(|(n, kind)| {
            sim.spawn_object_at_height(kind, "Americans", 10 + 4 * n as u16, 10, 0, 0, &rules)
                .unwrap_or_else(|| panic!("{kind} spawns"))
        })
        .collect();
    let bldg = sim.interner.get("BLDG").unwrap();
    let bldgd = sim.interner.get("BLDGD").unwrap();
    let tracking = &sim.houses[&house].tracking;
    assert_eq!(
        tracking.active_for_test(),
        (2, 1, 1),
        "the DontScore unit is added"
    );
    assert_eq!(tracking.active_count(EntityCategory::Structure, bldg), 1);
    assert_eq!(tracking.active_count(EntityCategory::Structure, bldgd), 0);

    for id in ids {
        sim.uninit_with_rules(id, &rules);
    }
    let tracking = &sim.houses[&house].tracking;
    assert_eq!(tracking.active_for_test(), (1, 0, 0), "but not removed");
    assert_eq!(tracking.active_count(EntityCategory::Structure, bldg), 0);
}

/// TechnoClass::ChangeOwner moves the tracking (`0x007015DE`, `0x007015E6`)
/// and, only for an object on the map, the on-map counts (`0x0070159D`,
/// `0x0070178E`, both behind the InLimbo test).
#[test]
fn change_owner_moves_the_counts() {
    let (mut sim, rules, first) = one_house();
    let second = sim.interner.intern("Russians");
    sim.houses
        .insert(second, HouseState::new(second, 1, None, false, 0, 10));
    let on_map = sim
        .spawn_object_at_height("TANK", "Americans", 10, 10, 0, 0, &rules)
        .unwrap();
    let in_limbo = sim
        .spawn_object_limbo_at_height("TANK", "Americans", 14, 10, 0, 0, &rules)
        .unwrap();
    let counts = |sim: &Simulation, house| {
        let tracking = &sim.houses[&house].tracking;
        (tracking.units_for_test(), tracking.active_for_test().0)
    };
    assert_eq!(counts(&sim, first), (2, 1));

    sim.change_owner(on_map, second);
    assert_eq!(
        (counts(&sim, first), counts(&sim, second)),
        ((1, 0), (1, 1))
    );
    sim.change_owner(in_limbo, second);
    assert_eq!(
        (counts(&sim, first), counts(&sim, second)),
        ((0, 0), (2, 1))
    );
}

/// A Helipad's `NumberOfDocks=` join its house's AirportDocks (`+0x2D4`) at
/// its first Grand_Opening (`0x004463C0`), not at a capture's; a capture
/// moves them, the old house unclamped (`0x00448B4C`, `0x00449229`); its
/// Limbo takes them back, clamped at zero (`0x00445946..0x00445988`).
#[test]
fn a_helipads_docks_follow_its_opening_capture_and_limbo() {
    let (mut sim, rules, first) = one_house();
    let second = sim.interner.intern("Russians");
    sim.houses
        .insert(second, HouseState::new(second, 1, None, false, 0, 10));
    let docks = |sim: &Simulation| {
        (
            sim.houses[&first].tracking.airport_docks(),
            sim.houses[&second].tracking.airport_docks(),
        )
    };
    let pad = sim
        .spawn_object_at_height("PAD", "Americans", 10, 10, 0, 0, &rules)
        .unwrap();
    assert_eq!(docks(&sim), (4, 0));
    let add_docks = |sim: &mut Simulation, house, docks| {
        sim.houses
            .get_mut(&house)
            .unwrap()
            .tracking
            .add_airport_docks(docks);
    };
    add_docks(&mut sim, first, -3);
    sim.change_owner_with_rules(pad, second, &rules, None);
    assert_eq!(docks(&sim), (-3, 4));
    add_docks(&mut sim, second, -2);
    sim.uninit_with_rules(pad, &rules);
    assert_eq!(docks(&sim), (-3, 0));
}

/// A constructed object that never leaves limbo (a cancelled build) is
/// deleted at once, and its destructor's Remove_Tracking goes with it.
#[test]
fn discarding_a_constructed_object_releases_its_tracking() {
    let (mut sim, rules, house) = one_house();
    let building = sim
        .spawn_object_limbo_at_height("BLDG", "Americans", 10, 10, 0, 0, &rules)
        .unwrap();
    let unit = sim
        .spawn_object_limbo_at_height("TANK", "Americans", 14, 10, 0, 0, &rules)
        .unwrap();
    let tracking = &sim.houses[&house].tracking;
    assert_eq!((tracking.buildings(), tracking.units_for_test()), (1, 1));

    assert!(sim.discard_constructed_limbo(building, Some(&rules)));
    assert!(sim.discard_constructed_limbo(unit, Some(&rules)));
    let tracking = &sim.houses[&house].tracking;
    assert_eq!((tracking.buildings(), tracking.units_for_test()), (0, 0));
    assert_eq!(tracking.active_for_test(), (0, 0, 0));
}

const VALUE_RULES: &str = "\
[Countries]
0=Americans
1=Russians
[Americans]
CostUnitsMult=0.5
[InfantryTypes]
0=GI
1=JUMP
[VehicleTypes]
0=TANK
1=TANKD
2=HOVER
3=CARRIER
[AircraftTypes]
0=PLANE
[BuildingTypes]
0=BLDG
[GI]
Strength=100
Cost=200
[JUMP]
Strength=100
Cost=600
ConsideredAircraft=yes
[TANK]
Strength=300
Cost=700
[TANKD]
Strength=300
Cost=900
DontScore=yes
[HOVER]
Strength=300
Cost=1000
ConsideredAircraft=yes
[CARRIER]
Strength=300
Cost=2000
Spawns=PLANE
[PLANE]
Strength=200
Cost=1500
[BLDG]
Strength=500
Cost=1000
";

fn force_values_of(sim: &Simulation, house: InternedId) -> (i32, i32, i32) {
    let values = sim.houses[&house].tracking.force_values();
    (values.infantry, values.vehicles, values.air)
}

/// The value arms of Added_To_Game and Removed_From_Game
/// (`0x00502B90..0x00502CE1`, `0x005028D0..0x00502A15`): an object's Cost_Of
/// for its house, with the country's `Cost*Mult=`, joins the infantry,
/// vehicle or air total while it is on the map, DontScore types too and
/// buildings never. `ConsideredAircraft=` moves an infantry or a unit to air,
/// `Spawns=` a unit; ChangeOwner prices it for each house in turn.
#[test]
fn force_values_follow_the_objects_on_the_map() {
    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str(VALUE_RULES),
        &IniFile::from_str("[BLDG]\nFoundation=2x2\n"),
    )
    .unwrap();
    let mut sim = Simulation::with_seed(3);
    let [americans, russians] = ["Americans", "Russians"].map(|name| {
        let house = sim.interner.intern(name);
        let mut state = HouseState::new(house, 0, None, false, 0, 10);
        state.project_country_mults(&rules, &sim.interner);
        sim.houses.insert(house, state);
        house
    });
    let ids: Vec<u64> = [
        "GI", "JUMP", "TANK", "TANKD", "HOVER", "CARRIER", "PLANE", "BLDG",
    ]
    .iter()
    .enumerate()
    .map(|(n, kind)| {
        sim.spawn_object_at_height(kind, "Americans", 10 + 4 * n as u16, 10, 0, 0, &rules)
            .unwrap_or_else(|| panic!("{kind} spawns"))
    })
    .collect();
    // Units at half price: TANK 350 + TANKD 450; air JUMP 600 + HOVER 500 +
    // CARRIER 1000 + PLANE 1500.
    assert_eq!(force_values_of(&sim, americans), (200, 800, 3600));

    sim.change_owner(ids[2], russians);
    assert_eq!(force_values_of(&sim, americans), (200, 450, 3600));
    assert_eq!(force_values_of(&sim, russians), (0, 700, 0));

    for id in ids {
        sim.uninit_with_rules(id, &rules);
    }
    assert_eq!(force_values_of(&sim, americans), (0, 0, 0));
    assert_eq!(force_values_of(&sim, russians), (0, 0, 0));
}

/// `Add_Tracking`'s `+0x2E8` and `+0x2F4` (`0x004FF72E`, `0x004FF7F3`): the
/// vehicles, with a 1x1 undeployer counted among them (`0x004FF78B`), and the
/// infantry; `Remove_Tracking` takes each back.
#[test]
fn vehicle_and_infantry_totals_follow_tracking() {
    let (mut sim, rules, house) = one_house();
    let ids: Vec<u64> = ["TANK", "GI", "BLDG", "B1X1", "PLANE"]
        .iter()
        .enumerate()
        .map(|(n, kind)| {
            sim.spawn_object_at_height(kind, "Americans", 10 + 4 * n as u16, 10, 0, 0, &rules)
                .unwrap_or_else(|| panic!("{kind} spawns"))
        })
        .collect();
    let tracking = &sim.houses[&house].tracking;
    assert_eq!(tracking.vehicles(), 2, "the tank and the 1x1 undeployer");
    assert_eq!(tracking.infantry(), 1);
    assert_eq!(tracking.buildings(), 1);
    // Remove_Tracking runs from the destructor at the pending-delete drain.
    for id in ids {
        sim.uninit_with_rules(id, &rules);
    }
    sim.flush_pending_delete();
    let tracking = &sim.houses[&house].tracking;
    assert_eq!((tracking.vehicles(), tracking.infantry()), (0, 0));
}
