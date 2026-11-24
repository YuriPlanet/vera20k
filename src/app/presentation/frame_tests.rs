use super::*;

/// A frame is captured for every object before each lent tick: its record
/// copies what drawing reads, not whole components. A new field that pushes
/// it past the ceiling copies more than drawing needs.
#[cfg(all(
    target_pointer_width = "64",
    not(any(feature = "phobos", feature = "debug-tools"))
))]
#[test]
fn the_draw_record_stays_under_its_ceiling() {
    let size = std::mem::size_of::<ObjectRecord>();
    assert!(size <= CEILING, "ObjectRecord is {size} bytes");
}

#[cfg(all(
    target_pointer_width = "64",
    not(any(feature = "phobos", feature = "debug-tools"))
))]
// 824 since an SHP Unit's draw reads three more of its fields: the body's
// current facing (`shp_body_facing`), the Foot occupation enable and
// IsOnCarryall. Four bytes of state, one alignment step of the record.
const CEILING: usize = 824;

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
    let order = frame.display_order(&sim);
    assert!(std::ptr::eq(order, frame.display_order(&sim)));
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
            7 => entity.seldom.selection.set_selected_for_test(true),
            12 => {
                entity.capture_manager = Some(Box::new(CaptureManagerState::with_victims_for_test(
                    1,
                    false,
                    &[25],
                )))
            }
            25 => entity.seldom.mind_control = MindControlLink::controlled_by_for_test(12),
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

/// An SHP Unit's body reads PrimaryFacing at the committed frame through
/// gamemd's Current (called at `0x0073C636`), or under Ares through its own
/// copy (`0x10048064..0x10048102`): on the first frame of a ROT 5 turn from
/// 0x4000 to 0 they answer 0x3FFC and 0x4000
/// (`tools/spatial_oracle/facing_class_ares.json`).
#[test]
fn an_shp_body_reads_primary_facing_through_the_games_current() {
    use crate::map::entities::EntityCategory;
    use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
    use crate::sim::{game_entity::GameEntity, movement::FacingClass};

    let ares = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=TANK\n[TANK]\nImmuneToAbduction=yes\n",
    ))
    .unwrap();
    assert!(ares.ares());
    let mut entity =
        GameEntity::test_default_of_category(1, "TANK", "Americans", 0, 0, EntityCategory::Unit);
    entity.body_facing = FacingClass::new(0, 5);
    entity.body_facing.snap(0x4000, 0);
    entity.body_facing.set(0, 0);
    assert_eq!(
        [RuleSet::empty_for_test(), &ares]
            .map(|rules| ObjectRecord::of_test_in(&entity, None, rules).shp_body_facing),
        [0x3FFC, 0x4000]
    );
}

/// A frame answers the screen questions for the objects the view reaches,
/// and keeps the radar facts for every object.
#[test]
fn a_frame_answers_for_the_view_and_keeps_every_radar_fact() {
    use crate::map::entities::EntityCategory;
    use crate::sim::{combat::Health, game_entity::GameEntity};

    let mut sim = Simulation::new();
    let owner = sim.intern("Americans");
    let tank = sim.intern("MTNK");
    for (id, cell) in [(3u64, 10u16), (7, 200)] {
        sim.entities_mut_for_test()
            .insert(GameEntity::new_at_frame_zero_for_test(
                id,
                cell,
                cell,
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
    let rules = crate::rules::ruleset::RuleSet::empty_for_test();
    let all = PresentationFrame::of_test(&sim, rules);
    assert!(
        all.objects().all(|record| record.is_scoped()),
        "a frame without a scope answers for every object"
    );
    let near = all.object(3).expect("the near tank");
    let span = crate::app::presentation::display_members::ScreenSpan::of(near);
    let window = crate::app::presentation::instances::PassByWindow::around([[
        span.x - 8.0,
        span.top - 8.0,
        span.x + 8.0,
        span.bottom + 8.0,
    ]]);
    let frame = PresentationFrame::of_test_with_scope(&sim, rules, Some((window, [0.0, 0.0])));

    let near = frame.object(3).expect("the near tank");
    assert!(near.is_scoped(), "the view reaches the near tank");
    assert_ne!(
        near.place.center_xy,
        [0, 0],
        "the near tank has a placement"
    );
    assert_ne!(near.hud.get_coords.x, 0, "the near tank has a status strip");

    let far = frame.object(7).expect("the far tank");
    assert!(!far.is_scoped(), "the view does not reach the far tank");
    assert_ne!(
        far.place.center_xy,
        [0, 0],
        "the far tank keeps the centre the radar places it by"
    );
    assert_ne!(
        far.hud.get_coords.x, 0,
        "the far tank keeps the coordinates the labels and rings place it by"
    );
    assert_ne!(
        far.scene.radar.location().x,
        0,
        "the far tank keeps its radar facts"
    );
}

/// The camera-range predicate accepts positions within its margin.
#[test]
fn a_frame_scope_covers_the_camera_by_the_margin() {
    use crate::map::entities::EntityCategory;
    use crate::sim::{combat::Health, game_entity::GameEntity};

    let mut sim = Simulation::new();
    let owner = sim.intern("Americans");
    let tank = sim.intern("MTNK");
    sim.entities_mut_for_test()
        .insert(GameEntity::new_at_frame_zero_for_test(
            3,
            10,
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
    let rules = crate::rules::ruleset::RuleSet::empty_for_test();
    let camera = [100.0f32, 200.0f32];
    let window = crate::app::presentation::instances::PassByWindow::around([[
        camera[0],
        camera[1],
        camera[0] + 800.0,
        camera[1] + 600.0,
    ]]);
    let frame = PresentationFrame::of_test_with_scope(&sim, rules, Some((window, camera)));
    assert!(frame.covers_camera(camera), "the scope covers its camera");
    assert!(
        frame.covers_camera([camera[0] + 100.0, camera[1] - 100.0]),
        "the scope covers a camera inside the margin"
    );
    assert!(
        !frame.covers_camera([camera[0] + 100_000.0, camera[1]]),
        "the scope does not cover a camera outside the margin"
    );
}
