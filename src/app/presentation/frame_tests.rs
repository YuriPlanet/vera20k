use super::*;

/// A frame is captured for every object before each lent tick: its record
/// copies what drawing reads, not whole components. A new field that pushes
/// it past the ceiling copies more than drawing needs.
#[cfg(all(
    target_pointer_width = "64",
    not(any(feature = "ares", feature = "phobos", feature = "debug-tools"))
))]
#[test]
fn the_draw_record_stays_under_its_ceiling() {
    let size = std::mem::size_of::<ObjectRecord>();
    assert!(size <= CEILING, "ObjectRecord is {size} bytes");
}

#[cfg(all(
    target_pointer_width = "64",
    not(any(feature = "ares", feature = "phobos", feature = "debug-tools"))
))]
const CEILING: usize = 816;

/// A record is found by its stable id and a missing id is not, and the
/// frame's Display order is built once: a second ask returns the first.
#[test]
fn a_frame_finds_records_by_id_and_keeps_one_display_order() {
    use crate::map::entities::EntityCategory;
    use crate::sim::{combat::Health, game_entity::GameEntity};

    let mut sim = Simulation::new();
    let owner = sim.intern("Americans");
    let tank = sim.intern("MTNK");
    for id in [3, 7, 20] {
        sim.entities_mut_for_test()
            .insert(GameEntity::new_at_frame_zero_for_test(
                id,
                10 + id as u16,
                10,
                0,
                0,
                owner,
                Health::new(100),
                tank,
                EntityCategory::Unit,
                0,
                5,
                false,
            ));
    }
    let frame = PresentationFrame::of_test(&sim, crate::rules::ruleset::RuleSet::empty_for_test());
    for id in [3, 7, 20] {
        assert_eq!(frame.object(id).map(|record| record.stable_id()), Some(id));
    }
    assert!(frame.object(8).is_none() && frame.object(21).is_none());
    let order = frame.display_order(sim.display_layers());
    assert!(std::ptr::eq(
        order,
        frame.display_order(sim.display_layers())
    ));
}

/// The building walk lists exactly the Structure records, in stable-id
/// order, between and after other objects.
#[test]
fn a_frame_lists_its_buildings_in_id_order() {
    use crate::map::entities::EntityCategory;
    use crate::sim::{combat::Health, game_entity::GameEntity};

    let mut sim = Simulation::new();
    let owner = sim.intern("Americans");
    let tank = sim.intern("MTNK");
    let yard = sim.intern("GACNST");
    for (id, kind, category) in [
        (3, tank, EntityCategory::Unit),
        (7, yard, EntityCategory::Structure),
        (20, tank, EntityCategory::Unit),
        (25, yard, EntityCategory::Structure),
    ] {
        sim.entities_mut_for_test()
            .insert(GameEntity::new_at_frame_zero_for_test(
                id,
                10 + id as u16,
                10,
                0,
                0,
                owner,
                Health::new(100),
                kind,
                category,
                0,
                5,
                false,
            ));
    }
    let frame = PresentationFrame::of_test(&sim, crate::rules::ruleset::RuleSet::empty_for_test());
    let buildings: Vec<u64> = frame
        .structures()
        .map(|record| record.stable_id())
        .collect();
    assert_eq!(buildings, [7, 25]);
}
