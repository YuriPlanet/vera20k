//! Type-select admission for structures.
//!
//! Retail's ID match (`0x00732C30`) admits a building only when its type has
//! `UndeploysInto=` (`0x00732C61..0x00732C83`). Phobos `[General]
//! BuildingTypeSelectable`, live only with Ares, lifts the type query of
//! CanBeSelectedNow for the ID match only, so every building that passes the
//! TechnoClass checks (`0x006FC030`) joins. These tests drive the tap that
//! reads those gates, not the gates alone.
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

/// The same pair with entity 2 holding a tank-bunker occupant, which
/// CanBeSelectedNow's TechnoClass checks (`0x006FC030`) refuse.
#[cfg(all(feature = "phobos", feature = "ares"))]
fn building_pair_with_garrison(type_id: &str) -> (Simulation, Vec<u64>) {
    let (mut sim, order) = two_buildings(type_id);
    sim.entities_mut_for_test()
        .get_mut(2)
        .expect("entity 2")
        .bunker
        .set_bunker_occupant_for_test(Some(99));
    (sim, order)
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
    assert!(
        !rules.structure_type_selectable("GAPOWR"),
        "the independent UndeploysInto arm stays retail"
    );
    assert_eq!(
        tap(sim.entities(), &order, &rules, sim.interner()),
        vec![1, 2]
    );
}

#[cfg(all(feature = "phobos", feature = "ares"))]
#[test]
fn the_switch_still_applies_the_building_selection_checks() {
    let (sim, order) = building_pair_with_garrison("GAPOWR");
    let rules = rules("BuildingTypeSelectable=yes\n");
    assert_eq!(
        tap(sim.entities(), &order, &rules, sim.interner()),
        vec![1],
        "the hook lifts only Is1x1AndUndeployable 0x465D40, so CanBeSelectedNow \
         still refuses the occupied bunker"
    );
}

#[cfg(all(feature = "phobos", feature = "ares"))]
#[test]
fn the_undeploysinto_arm_still_outranks_the_building_checks() {
    let (sim, order) = building_pair_with_garrison("GACNST");
    let rules = rules("BuildingTypeSelectable=yes\n");
    assert_eq!(
        tap(sim.entities(), &order, &rules, sim.interner()),
        vec![1, 2],
        "0x00732C61..0x00732C83 admits the UndeploysInto type on its own"
    );
}

#[cfg(all(feature = "phobos", feature = "ares"))]
#[test]
fn the_switch_lifts_only_the_id_match_context() {
    // Phobos sets ExtSelection::ProcessingIDMatches around the ID match only
    // (hooks 0x732A85/0x732B28, cleared at 0x732C06), so the band and N/M
    // walks keep CanBeSelectedNow's own type query.
    let (sim, _) = two_buildings("GAPOWR");
    let rules = rules("BuildingTypeSelectable=yes\n");
    let entity = sim.entities().get(1).expect("entity 1");
    assert!(
        !super::selection_dynamic_prefix(
            entity,
            sim.entities(),
            Some(&rules),
            Some(sim.interner()),
            false,
        ),
        "outside the ID match a plain building is still refused"
    );
    assert!(super::selection_dynamic_prefix(
        entity,
        sim.entities(),
        Some(&rules),
        Some(sim.interner()),
        true,
    ));
}

#[cfg(all(feature = "phobos", feature = "ares"))]
#[test]
fn the_switch_is_read_from_the_general_section() {
    // Absent key keeps the retail gate.
    assert!(!rules("").building_type_selectable());
    assert!(!rules("").structure_type_selectable("GAPOWR"));
    // `no` is the same as absent.
    assert!(!rules("BuildingTypeSelectable=no\n").building_type_selectable());
    assert!(rules("BuildingTypeSelectable=yes\n").building_type_selectable());
}
