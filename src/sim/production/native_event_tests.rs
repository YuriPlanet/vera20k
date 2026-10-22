//! Native EventClass selectors exercised through the production receivers.
use super::*;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::{
    Command, CommandEnvelope, CommandRecord, PlaceRecord, ProductionEvent, ProductionRecord,
};
use crate::sim::intern::InternedId;
use crate::sim::world::{FrameEffects, Simulation};

fn event(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    kind: ProductionEvent,
    name: &str,
) -> CommandRecord {
    sim.encode_production_record(owner, kind, sim.interner().get(name).unwrap(), rules)
        .unwrap()
}

fn apply(sim: &mut Simulation, rules: &RuleSet, record: CommandRecord) -> bool {
    sim.apply_command(
        "ignored carrier owner",
        &Command::SynchronizedRecord { record },
        Some(rules),
    )
}

fn held(sim: &Simulation, owner: InternedId, category: ProductionCategory) -> u64 {
    sim.production
        .factories()
        .view(owner, category)
        .unwrap()
        .object
        .unwrap()
        .entity_id
        .unwrap()
}

#[test]
fn mobile_place_encoder_uses_the_native_empty_cell_and_current_object_kind() {
    let rules = tests::basic_multi_queue_rules();
    let mut sim = Simulation::new();
    tests::spawn_structure(&mut sim, &rules, 1, "Americans", "GAPILE", 10, 10);
    sim.intern_rule_type_ids(&rules);
    let owner = sim.interner().get("Americans").unwrap();
    let type_id = sim.interner().get("E1").unwrap();
    complete_held_building_for_test(&mut sim, &rules, owner, type_id);
    let record = sim
        .encode_place_mobile_record(owner, ProductionCategory::Infantry, &rules)
        .unwrap();
    let view = PlaceRecord::decode(&record).unwrap();
    // Strip6A8EBA and cameo6AB3E1 use B07DD8, zeroed at6A4BC2/6A4BC8.
    assert_eq!(
        (view.rtti, view.type_index, view.naval, view.x, view.y),
        (15, -1, 0, 0, 0)
    );
    // Completed-cameo6AB3EF instead retains the clicked type's RTTI.
    let mut clicked = ProductionSelection::for_type(&rules, rules.object("E1").unwrap()).unwrap();
    clicked.type_index = -1;
    let record = sim.encode_place_record(owner, clicked, (0, 0)).unwrap();
    assert_eq!(PlaceRecord::decode(&record).unwrap().rtti, 16);
}

#[test]
fn native_family_aliases_keep_build_cat_and_naval_selection_independent() {
    use crate::rules::object_type::ObjectCategory;
    let rules = tests::build_catalog_rules();
    // Original48DCD0's four object/type pairs, then4FA942's category switch.
    // The Unit pair consults packet Naval; only Building resolves BuildCat.
    for (pair, family, name, expected) in [
        (
            [1, 40],
            ObjectCategory::Vehicle,
            "MTNK",
            ProductionCategory::Vehicle,
        ),
        (
            [2, 3],
            ObjectCategory::Aircraft,
            "ORCA",
            ProductionCategory::Aircraft,
        ),
        (
            [6, 7],
            ObjectCategory::Building,
            "GTUR",
            ProductionCategory::Defense,
        ),
        (
            [15, 16],
            ObjectCategory::Infantry,
            "E1",
            ProductionCategory::Infantry,
        ),
    ] {
        let type_index = rules.type_array_index(family, name).unwrap();
        for rtti in pair {
            let selection = ProductionSelection {
                rtti,
                type_index,
                naval: false,
            };
            assert_eq!(selection.object_type(&rules).unwrap().id, name);
            assert_eq!(selection.category(&rules), Some(expected));
            let naval = ProductionSelection {
                naval: true,
                ..selection
            };
            assert_eq!(
                naval.category(&rules),
                Some(if family == ObjectCategory::Vehicle {
                    ProductionCategory::Ship
                } else {
                    expected
                })
            );
        }
    }
    for rtti in [6, 7] {
        assert_eq!(
            ProductionSelection {
                rtti,
                type_index: -1,
                naval: true
            }
            .category(&rules),
            Some(ProductionCategory::Building)
        );
    }
    assert!(
        ProductionSelection {
            rtti: 5,
            type_index: 0,
            naval: false
        }
        .category(&rules)
        .is_none()
    );
}

#[test]
fn production_raw_frame_gates_begin_then_hold_resume_and_cancel_share_the_held_identity() {
    let rules = tests::basic_multi_queue_rules();
    let mut sim = Simulation::new();
    tests::spawn_structure(&mut sim, &rules, 1, "Americans", "GAPILE", 10, 10);
    sim.intern_rule_type_ids(&rules);
    let owner = sim.interner().get("Americans").unwrap();
    let mut begin = event(&sim, &rules, owner, ProductionEvent::Produce, "E1");
    begin.set_frame_stamp(10);
    sim.session.set_binary_frame_for_test(9);
    let envelope = CommandEnvelope::synchronized(begin, 0);
    assert_eq!(
        sim.apply_due_commands(&[envelope], &rules, 1, None, FrameEffects::default())
            .0,
        0
    );
    assert!(
        sim.production
            .factories()
            .view(owner, ProductionCategory::Infantry)
            .is_none()
    );
    sim.session.set_binary_frame_for_test(10);
    assert_eq!(
        sim.apply_due_commands(&[], &rules, 1, None, FrameEffects::default())
            .0,
        1
    );
    let id = held(&sim, owner, ProductionCategory::Infantry);
    let suspend = event(&sim, &rules, owner, ProductionEvent::Suspend, "E1");
    assert!(apply(&mut sim, &rules, suspend));
    assert!(
        sim.production
            .factories()
            .view(owner, ProductionCategory::Infantry)
            .unwrap()
            .suspended
    );
    let resume = event(&sim, &rules, owner, ProductionEvent::Produce, "E1");
    assert!(apply(&mut sim, &rules, resume.clone()));
    assert_eq!(held(&sim, owner, ProductionCategory::Infantry), id);
    assert!(
        !sim.production
            .factories()
            .view(owner, ProductionCategory::Infantry)
            .unwrap()
            .suspended
    );
    assert!(apply(&mut sim, &rules, resume));
    assert_eq!(
        sim.production
            .factories()
            .view(owner, ProductionCategory::Infantry)
            .unwrap()
            .queue
            .len(),
        1
    );
    let abandon = event(&sim, &rules, owner, ProductionEvent::Abandon, "E1");
    assert!(apply(&mut sim, &rules, abandon));
    assert_eq!(held(&sim, owner, ProductionCategory::Infantry), id);
    assert!(
        sim.production
            .factories()
            .view(owner, ProductionCategory::Infantry)
            .unwrap()
            .queue
            .is_empty()
    );
    let abandon_all = event(&sim, &rules, owner, ProductionEvent::AbandonAll, "E1");
    assert!(apply(&mut sim, &rules, abandon_all));
    assert!(
        sim.production
            .factories()
            .view(owner, ProductionCategory::Infantry)
            .is_none()
    );
    assert!(!sim.entities().contains(id));
}

#[test]
fn independent_naval_and_negative_wildcard_select_the_native_factory_slot() {
    let rules = tests::basic_multi_queue_rules();
    let mut sim = Simulation::new();
    tests::spawn_structure(&mut sim, &rules, 1, "Americans", "GAWEAP", 10, 10);
    sim.intern_rule_type_ids(&rules);
    let owner = sim.interner().get("Americans").unwrap();
    let mut raw = ProductionRecord::decode(&event(
        &sim,
        &rules,
        owner,
        ProductionEvent::Produce,
        "MTNK",
    ))
    .unwrap();
    raw.naval = 256; // Execute4C7128 tests the entire DWORD, not its low byte.
    let mut record =
        CommandRecord::decode_exact(&[0; crate::sim::command::COMMAND_RECORD_LEN]).unwrap();
    raw.write_into(&mut record);
    assert!(apply(&mut sim, &rules, record.clone()));
    let id = held(&sim, owner, ProductionCategory::Ship);
    assert!(
        sim.production
            .factories()
            .view(owner, ProductionCategory::Vehicle)
            .is_none()
    );
    raw.event = ProductionEvent::Suspend;
    raw.rtti = 1; // object/type RTTI aliases select the same slot
    raw.type_index = -1;
    raw.write_into(&mut record);
    assert!(apply(&mut sim, &rules, record.clone()));
    assert!(
        sim.production
            .factories()
            .view(owner, ProductionCategory::Ship)
            .unwrap()
            .suspended
    );
    raw.event = ProductionEvent::Abandon;
    raw.type_index = -2;
    raw.write_into(&mut record);
    assert!(!apply(&mut sim, &rules, record.clone()));
    assert_eq!(held(&sim, owner, ProductionCategory::Ship), id);
    raw.type_index = -1;
    raw.write_into(&mut record);
    assert!(apply(&mut sim, &rules, record));
    assert!(
        sim.production
            .factories()
            .view(owner, ProductionCategory::Ship)
            .is_none()
    );
}

#[test]
fn exact_building_type_rtti_alone_refuses_a_second_running_produce() {
    let rules = tests::placement_radius_rules();
    let mut sim = Simulation::new();
    tests::spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 10, 10);
    sim.intern_rule_type_ids(&rules);
    let owner = sim.interner().get("Americans").unwrap();
    let begin = event(&sim, &rules, owner, ProductionEvent::Produce, "GAPOWR");
    assert!(apply(&mut sim, &rules, begin));
    let first = held(&sim, owner, ProductionCategory::Building);
    let mut replacement = event(&sim, &rules, owner, ProductionEvent::Produce, "GAGAP");
    assert!(!apply(&mut sim, &rules, replacement.clone()));
    let mut view = ProductionRecord::decode(&replacement).unwrap();
    view.rtti = 6;
    view.write_into(&mut replacement);
    assert!(apply(&mut sim, &rules, replacement));
    let second = held(&sim, owner, ProductionCategory::Building);
    assert_ne!(first, second);
    assert!(!sim.entities().contains(first));
    assert_eq!(
        sim.interner()
            .resolve(sim.entities().get(second).unwrap().type_ref()),
        "GAGAP"
    );
}

#[test]
fn place_uses_the_current_held_type_and_retains_original_selector_for_settlement() {
    let rules = tests::placement_radius_rules();
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    tests::spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 10, 10);
    sim.intern_rule_type_ids(&rules);
    let owner = sim.interner().get("Americans").unwrap();
    let type_id = sim.interner().get("GAPOWR").unwrap();
    let id = complete_held_building_for_test(&mut sim, &rules, owner, type_id);
    let requested = sim.interner().get("GAGAP").unwrap();
    let record = sim
        .encode_place_building_record(owner, requested, (12, 10), &rules)
        .unwrap();
    assert_ne!(
        PlaceRecord::decode(&record).unwrap().type_index,
        rules
            .type_array_index(
                crate::rules::object_type::ObjectCategory::Building,
                "GAPOWR"
            )
            .unwrap()
    );
    let envelope = CommandEnvelope::synchronized(record, u64::MAX);
    let (count, changed) =
        sim.apply_due_commands(&[envelope], &rules, 1, None, FrameEffects::default());
    assert_eq!(count, 1);
    assert!(
        changed,
        "raw PLACE must request the ordinary world/vision refresh"
    );
    let placed = sim.entities().get(id).unwrap();
    assert!(!placed.lifecycle.in_limbo());
    assert_eq!((placed.position.rx(), placed.position.ry()), (12, 10));
    assert_eq!(sim.interner().resolve(placed.type_ref()), "GAPOWR");
    // Both authored types have BuildCat0. The original GAGAP index is retained
    // after Completed clears the held object: indexed null-held Abandon refuses.
    assert!(
        sim.production
            .factories()
            .view(owner, ProductionCategory::Building)
            .is_some()
    );
}
