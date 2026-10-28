//! Type-select admission for structures.
//!
//! Retail's ID match (`0x00732C30`) admits a building only when its type has
//! `UndeploysInto=` (`0x00732C61..0x00732C83`). Phobos
//! `[General] BuildingTypeSelectable`, live only with Ares, admits every
//! building. These tests drive the tap that reads that gate, not the gate
//! alone.
use crate::app::input::entity_pick::compute_type_select_tap_with_playfield;
use crate::app::types::TypeSelectOutcome;
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::Health;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;

/// One ordinary building (`GAPOWR`, no `UndeploysInto`) and one construction
/// yard that undeploys into its vehicle form.
fn rules(general: &str) -> RuleSet {
    let ini = format!(
        "[General]\n{general}\
         [BuildingTypes]\n0=GAPOWR\n1=GACNST\n\
         [VehicleTypes]\n0=AMCV\n\
         [GAPOWR]\nStrength=750\n\
         [GACNST]\nStrength=1000\nUndeploysInto=AMCV\n\
         [AMCV]\nStrength=200\n"
    );
    RuleSet::from_ini(&IniFile::from_str(&ini)).expect("rules read")
}

fn structure(id: u64, owner: InternedId, type_ref: InternedId) -> GameEntity {
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        id,
        10 + id as u16,
        10,
        0,
        0,
        owner,
        Health::new(100),
        type_ref,
        EntityCategory::Structure,
        0,
        5,
        false,
    );
    entity.lifecycle.set_object_alive_for_test(true);
    entity.lifecycle.set_in_limbo_for_test(false);
    entity
}

/// `(sim, order)`, with entity 1 selected and entity 2 of the same type.
fn two_buildings(type_id: &str) -> (Simulation, Vec<u64>) {
    let mut sim = Simulation::new();
    let owner = sim.intern("me");
    let ty = sim.intern(type_id);
    for id in [1u64, 2] {
        sim.entities_mut_for_test().insert(structure(id, owner, ty));
    }
    (sim, vec![1, 2])
}

/// Run the tap the way a second press of T does with the map scope already
/// chosen, so the result is the ID match alone.
fn tap(
    entities: &EntityStore,
    order: &[u64],
    rules: &RuleSet,
    interner: &crate::sim::intern::StringInterner,
) -> Vec<u64> {
    let result = compute_type_select_tap_with_playfield(
        entities,
        order,
        order,
        &[1],
        None,
        Some("me"),
        Some(rules),
        Some(interner),
        true,
        false,
    );
    assert_eq!(result.outcome, TypeSelectOutcome::Map);
    result.mutation.select
}

#[test]
fn a_plain_building_is_not_a_type_select_seed_without_the_switch() {
    let (sim, order) = two_buildings("GAPOWR");
    let rules = rules("");
    assert!(!rules.structure_type_selectable("GAPOWR"));
    assert!(
        tap(sim.entities(), &order, &rules, sim.interner()).is_empty(),
        "the retail gate admits no plain building"
    );
}

#[test]
fn an_undeploysinto_building_stays_admissible() {
    let (sim, order) = two_buildings("GACNST");
    let rules = rules("");
    assert!(rules.structure_type_selectable("GACNST"));
    assert_eq!(
        tap(sim.entities(), &order, &rules, sim.interner()),
        vec![1, 2]
    );
}

#[cfg(all(feature = "phobos", feature = "ares"))]
#[test]
fn the_switch_admits_every_building() {
    let (sim, order) = two_buildings("GAPOWR");
    let rules = rules("BuildingTypeSelectable=yes\n");
    assert!(rules.structure_type_selectable("GAPOWR"));
    assert_eq!(
        tap(sim.entities(), &order, &rules, sim.interner()),
        vec![1, 2]
    );
}

#[cfg(all(feature = "phobos", feature = "ares"))]
#[test]
fn the_switch_is_read_from_the_general_section() {
    // Absent key keeps the retail gate.
    assert!(!rules("").structure_type_selectable("GAPOWR"));
    // `no` is the same as absent.
    assert!(!rules("BuildingTypeSelectable=no\n").structure_type_selectable("GAPOWR"));
    assert!(rules("BuildingTypeSelectable=yes\n").structure_type_selectable("GAPOWR"));
}
