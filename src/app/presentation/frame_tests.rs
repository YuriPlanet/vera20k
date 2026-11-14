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

/// The line walks visit exactly the records that can draw a tactical line
/// (`TacticalClass_Draw`'s Techno walk), in stable-id order: a selected
/// object, a mind-control manager and its captive, but not the idle objects
/// between them. An airstrike designation or psychic detection lists a
/// record too.
#[test]
fn a_frame_lists_the_objects_that_can_draw_a_line() {
    use crate::map::entities::EntityCategory;
    use crate::sim::capture_manager::{CaptureManagerState, MindControlLink};
    use crate::sim::{combat::Health, game_entity::GameEntity};

    let mut sim = Simulation::new();
    let owner = sim.intern("Americans");
    let tank = sim.intern("MTNK");
    for id in [3, 7, 12, 20, 25] {
        let mut entity = GameEntity::new_at_frame_zero_for_test(
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
        );
        match id {
            7 => entity.selection.set_selected_for_test(true),
            12 => {
                entity.capture_manager = Some(Box::new(CaptureManagerState::with_victims_for_test(
                    1,
                    false,
                    &[25],
                )))
            }
            25 => entity.mind_control = MindControlLink::controlled_by_for_test(12),
            _ => {}
        }
        sim.entities_mut_for_test().insert(entity);
    }
    let frame = PresentationFrame::of_test(&sim, crate::rules::ruleset::RuleSet::empty_for_test());
    let sources: Vec<u64> = frame
        .line_sources()
        .map(|record| record.stable_id())
        .collect();
    assert_eq!(sources, [7, 12, 25]);

    let mut detected = frame.object(3).expect("captured").clone();
    assert!(!detected.may_draw_a_line());
    detected.place.psychically_detected = true;
    assert!(detected.may_draw_a_line());
    let mut designating = frame.object(20).expect("captured").clone();
    let origin = crate::sim::components::DriveCoord { x: 0, y: 0, z: 0 };
    designating.place.airstrike_designation = Some(AirstrikeDesignation {
        coords: [origin; 2],
        #[cfg(feature = "phobos")]
        color: [0; 3],
    });
    assert!(designating.may_draw_a_line());
}
