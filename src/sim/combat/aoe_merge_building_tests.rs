//! Phobos `MergeBuildingDamage` (`Ext/WarheadType/Hooks.cpp:410-487`).

use super::{AoEDamageResult, AreaDamageReceiver, merge_building_records};
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::receiver_call::EntityDamageEvent;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::test_intern;

const VERSES: &str = "Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n";
const BUILDING: u64 = 7;
const VEHICLE: u64 = 9;

fn rules() -> RuleSet {
    let ini = IniFile::from_str(&format!(
        "[InfantryTypes]\n[VehicleTypes]\n0=MTNK\n[AircraftTypes]\n[BuildingTypes]\n0=GAPOWR\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\n\
         [GAPOWR]\nStrength=750\nArmor=wood\n\
         [Warheads]\n0=WH\n\
         [WH]\n{VERSES}"
    ));
    RuleSet::from_ini(&ini).expect("the fixture rules parse")
}

/// A building beside a vehicle: the sweep gives the building one record per
/// foundation cell it reaches and the vehicle one per cell it stands in.
fn entities() -> EntityStore {
    let mut entities = EntityStore::new();
    entities.insert(GameEntity::test_default_of_category(
        BUILDING,
        "GAPOWR",
        "Americans",
        4,
        4,
        EntityCategory::Structure,
    ));
    entities.insert(GameEntity::test_default(VEHICLE, "MTNK", "Americans", 6, 4));
    entities
}

fn record(target: u64, distance: i32) -> EntityDamageEvent {
    EntityDamageEvent::area(target, 100, distance, 1, None, test_intern("WH"))
}

fn result_with(events: &[EntityDamageEvent]) -> AoEDamageResult {
    let mut result = AoEDamageResult::default();
    for event in events {
        result.receivers.push(AreaDamageReceiver::Entity(*event));
        result.hits.push(*event);
    }
    result
}

/// `Ext/WarheadType/Hooks.cpp:426-478`: the four foundation records collapse
/// into one call at distance 0, and the vehicle keeps both of its records.
#[test]
fn four_building_records_become_one() {
    let entities = entities();
    let rules = rules();
    let wh = rules.warhead("WH").expect("WH parses");
    let events = [
        record(BUILDING, 0),
        record(BUILDING, 40),
        record(VEHICLE, 40),
        record(BUILDING, 40),
        record(VEHICLE, 80),
        record(BUILDING, 40),
    ];
    let mut result = result_with(&events);
    // CellSpread 0 leaves every multiplier at 1.0, so the sum is 4 and the
    // merged packet is 400.
    merge_building_records(&mut result, &entities, wh, 0, 100);

    let events: Vec<EntityDamageEvent> = result
        .receivers
        .iter()
        .filter_map(|receiver| match receiver {
            AreaDamageReceiver::Entity(event) => Some(*event),
            AreaDamageReceiver::Terrain(_) => None,
        })
        .collect();
    assert_eq!(
        events.len(),
        3,
        "one merged building record, two vehicle ones"
    );
    assert_eq!(events[0].target_id, BUILDING);
    assert_eq!(events[0].damage, 400);
    assert_eq!(events[0].distance_leptons, 0);
    assert_eq!(events[1].target_id, VEHICLE);
    assert_eq!(events[1].damage, 100);
    assert_eq!(events[2].target_id, VEHICLE);
    assert_eq!(result.hits.len(), 3, "the compatibility projection follows");
}

/// The per-record term is `1.0 - (1 - PercentAtMax) * distance / cellSpread`,
/// a record past the spread contributes nothing, and a product that truncates
/// to zero keeps the base damage's sign.
#[test]
fn the_distance_term_is_summed_inside_the_spread_only() {
    let entities = entities();
    let rules = rules();
    let mut warhead = rules.warhead("WH").expect("WH parses").clone();
    warhead.percent_at_max_f64 = 0.2;
    let events = [
        record(BUILDING, 0),
        record(BUILDING, 320),
        // Past the 512-lepton spread: the accumulation loop skips it.
        record(BUILDING, 1024),
    ];
    let merged = |base_damage: i32| {
        let mut result = result_with(&events);
        merge_building_records(&mut result, &entities, &warhead, 512, base_damage);
        assert_eq!(result.hits.len(), 1, "one record per building");
        result.hits[0].damage
    };
    // 1.0 + 0.5 = 1.5.
    assert_eq!(merged(10), 15);
    assert_eq!(merged(1), 1);
    assert_eq!(merged(-1), -1);
}

/// A building the sweep reached once is rewritten to distance 0 without
/// changing its packet.
#[test]
fn a_single_record_is_kept_and_zeroed() {
    let entities = entities();
    let rules = rules();
    let wh = rules.warhead("WH").expect("WH parses");
    let mut result = result_with(&[record(BUILDING, 128)]);
    merge_building_records(&mut result, &entities, wh, 512, 42);
    assert_eq!(result.hits.len(), 1);
    assert_eq!(result.hits[0].damage, 42);
    assert_eq!(result.hits[0].distance_leptons, 0);
}
