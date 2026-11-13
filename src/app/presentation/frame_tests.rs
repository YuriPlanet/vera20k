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

/// The next capture fills the last frame's record storage instead of
/// allocating its own.
#[test]
fn a_capture_fills_the_last_frames_record_storage() {
    let mut simulation = Simulation::new();
    for id in 1..=3 {
        simulation
            .entities_mut_for_test()
            .insert(GameEntity::test_default(
                id,
                "MTNK",
                "Americans",
                10 + id as u16,
                10,
            ));
    }
    *simulation.interner_mut_for_test() = crate::sim::intern::test_interner();
    let runtime = SimRuntime {
        simulation,
        resources: std::sync::Arc::new(crate::sim::runtime::SimResources::new(
            crate::rules::ruleset::RuleSet::empty_for_test().clone(),
        )),
    };
    let records = PresentationFrame::capture(&runtime).into_records();
    let storage = records.as_ptr();
    let frame = PresentationFrame::capture_into(&runtime, records);
    assert_eq!(frame.objects().len(), 3);
    assert_eq!(frame.objects.as_ptr(), storage);
}
