//! Tests for the house counts. `native_tracking_corpus` compares against
//! `tools/spatial_oracle/house_tracking.json`, produced by running the
//! original Add_Tracking and Remove_Tracking under Unicorn.

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::HouseState;
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
Foundation=2x2
[BLDGI]
Strength=500
Foundation=2x2
Insignificant=yes
[BLDGD]
Strength=500
Foundation=2x2
DontScore=yes
[B1X1]
Strength=500
Foundation=1x1
UndeploysInto=TANK
[BGAT]
Strength=500
Foundation=2x2
UndeploysInto=HARV
[BOTHER]
Strength=500
Foundation=2x2
UndeploysInto=TANK
";

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
    let cases: Vec<NativeTrackingCase> = serde_json::from_str(include_str!(
        "../../tools/spatial_oracle/house_tracking.json"
    ))
    .unwrap();
    assert_eq!(cases.len(), 36);
    let rules = RuleSet::from_ini(&IniFile::from_str(RULES)).unwrap();
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
            (tracking.buildings_for_test(), tracking.units_for_test())
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
