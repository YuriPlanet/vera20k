//! House notification predicates and their existing production writers.
use super::tests::{
    event_ingress_fixture, event_ingress_fixture_with_rules, install_event_ingress_map,
};
use super::*;
use crate::rules::ruleset::RuleSet;

fn install(sim: &mut Simulation, rules: &RuleSet, kind: i32, country: i32) -> (usize, usize) {
    install_event_ingress_map(
        sim,
        rules,
        &format!(
            "[Triggers]\nNOTIFY=Country0,<none>,Notify,0,1,1,1,0\n\
             [Events]\nNOTIFY=1,{kind},0,{country}\n\
             [Actions]\nNOTIFY=1,28,0,7,0,0,0,0,A\n[Tags]\nTAG=2,Notify,NOTIFY\n"
        ),
    );
    let typ = sim.trigger_runtime.definitions.tag_type("TAG").unwrap();
    let tag = sim.shared_trigger_tag(typ);
    (tag, sim.trigger_runtime.tag(tag).unwrap().triggers[0])
}

#[test]
fn house_notification_predicates_replay_twenty_original_controls() {
    // Original HasOccurred71E940/71EC9A/71F069/71F0E1/71F0FB and502D30,
    // Steam gamemd SHA2563e81a617..08600. Twenty original calls through
    // PowerEventControls/FlagsFixture, with counted TEvent readers, supplied
    // House flags, unchanged code and no gameplay/scene substitution claim.
    // Expected result pairs are [event3,event5]; raised None means that kind.
    let cases = [
        (
            "clear",
            0,
            [false; 3],
            [0, 1, 2],
            Some(0),
            None,
            false,
            false,
            [false, false],
        ),
        (
            "set",
            0,
            [true, false, false],
            [0, 1, 2],
            Some(1),
            None,
            false,
            false,
            [true, true],
        ),
        (
            "duplicate_first_clear",
            0,
            [false, false, true],
            [0, 1, 2],
            Some(1),
            None,
            false,
            false,
            [false, false],
        ),
        (
            "duplicate_reordered_set",
            0,
            [false, false, true],
            [2, 1, 0],
            Some(1),
            None,
            false,
            false,
            [true, true],
        ),
        (
            "null_incoming",
            7,
            [false, true, false],
            [0, 1, 2],
            None,
            None,
            false,
            false,
            [true, true],
        ),
        (
            "unknown_country",
            999,
            [false; 3],
            [0, 1, 2],
            Some(0),
            None,
            false,
            false,
            [true, true],
        ),
        (
            "bypass_flag",
            7,
            [false, true, false],
            [0, 1, 2],
            Some(0),
            None,
            true,
            false,
            [false, true],
        ),
        (
            "retain_persistence",
            7,
            [false; 3],
            [0, 1, 2],
            Some(0),
            None,
            false,
            true,
            [false, false],
        ),
        (
            "house_poll8",
            0,
            [true; 3],
            [0, 1, 2],
            Some(0),
            Some(8),
            false,
            false,
            [false, true],
        ),
        (
            "wrong_raise47",
            0,
            [true; 3],
            [0, 1, 2],
            Some(0),
            Some(47),
            false,
            false,
            [false, true],
        ),
    ];
    for (column, kind) in [3, 5].into_iter().enumerate() {
        for (name, country, flags, order, incoming, raised, bypass, prior, expected) in cases {
            let (mut sim, rules, _, houses) = event_ingress_fixture();
            for (house, flag) in houses.into_iter().zip(flags) {
                if flag {
                    let house = sim.houses.get_mut(&house).unwrap();
                    if kind == 3 {
                        house.notify_building_capture();
                    } else {
                        house.notify_discovered_by_current_house();
                    }
                }
            }
            sim.houses
                .reorder_for_test(&order.map(|i| houses[i]).to_vec());
            let (_, live) = install(&mut sim, &rules, kind, country);
            let type_index = sim.trigger_runtime.trigger(live).unwrap().type_index;
            sim.trigger_runtime.definitions.trigger_types[type_index].events[0].retained_house =
                Some(houses[1]);
            let timer = sim.trigger_runtime.trigger(live).unwrap().timer;
            let before = sim.state_hash();
            let rng = sim.rng_state();
            let mut persistence = prior;
            let mut predicate = |sim: &mut Simulation| {
                sim.has_occurred(
                    0,
                    TagEventInput {
                        kind: raised.unwrap_or(kind),
                        ..Default::default()
                    },
                    incoming.map(|i| houses[i]),
                    &mut persistence,
                    Some(&rules),
                    live,
                )
            };
            let actual = if bypass {
                sim.with_buildup_bypass(predicate)
            } else {
                predicate(&mut sim)
            };
            assert_eq!(actual, expected[column], "event{kind}/{name}");
            assert_eq!(persistence, prior, "event{kind}/{name}");
            assert_eq!(sim.trigger_runtime.trigger(live).unwrap().timer, timer);
            assert_eq!(
                sim.trigger_runtime.definitions.trigger_types[type_index].events[0].retained_house,
                Some(houses[1])
            );
            assert_eq!(sim.state_hash(), before);
            assert_eq!(sim.rng_state(), rng);
        }
    }
}

const ENTRY_RULES: &str = "[InfantryTypes]\n0=ENGINEER\n\
    [ENGINEER]\nStrength=100\nSpeed=5\nSpeedType=Foot\nEngineer=yes\n\
    Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
    [BuildingTypes]\n0=DEPOT\n[DEPOT]\nStrength=500\nFoundation=1x1\nCapturable=yes\n";

#[test]
fn discovered_house_event_runs_from_real_discovery_poll_and_saved_notification() {
    let (mut sim, rules, registry, houses) = event_ingress_fixture_with_rules(ENTRY_RULES);
    let building = sim
        .spawn_object("DEPOT", "House1", 16, 16, 0, &rules)
        .unwrap();
    sim.session.set_current_house_for_test(Some(houses[0]));
    let (_, live) = install(&mut sim, &rules, 5, 7);
    assert!(!sim.houses[&houses[1]].discovered_by_current_house());
    sim.session.set_binary_frame_for_test(8);
    sim.raise_house_tags(
        houses[0],
        Some(&rules),
        Some(&registry),
        FrameEffects::default(),
    );
    assert!(!sim.trigger_runtime.globals_set.contains(&7));

    // TechnoDiscovered6F4A25 writes the object's owner, independently of
    // the Trigger's incoming House0. No new discovery writer is introduced.
    assert!(sim.discover_techno(
        building,
        houses[0],
        crate::sim::world::UninitContext::new(Some(&rules), Some(&registry))
    ));
    assert!(sim.houses[&houses[1]].discovered_by_current_house());
    assert!(!sim.houses[&houses[0]].discovered_by_current_house());
    let rng = sim.rng_state();
    sim.session.set_binary_frame_for_test(9);
    sim.raise_house_tags(
        houses[0],
        Some(&rules),
        Some(&registry),
        FrameEffects::default(),
    );
    assert!(!sim.trigger_runtime.globals_set.contains(&7));
    sim.session.set_binary_frame_for_test(16);
    sim.raise_house_tags(
        houses[0],
        Some(&rules),
        Some(&registry),
        FrameEffects::default(),
    );
    assert!(sim.trigger_runtime.globals_set.contains(&7));
    assert_eq!(sim.trigger_runtime.trigger(live).unwrap().completed, 0);
    assert_eq!(
        sim.trigger_runtime.trigger(live).unwrap().retained_house,
        None
    );
    assert_eq!(sim.rng_state(), rng);

    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "House notification", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    restored.restore_after_snapshot_load().unwrap();
    assert!(restored.houses[&houses[1]].discovered_by_current_house());
    assert!(restored.set_trigger_variable(true, 7, false));
    restored.session.set_binary_frame_for_test(24);
    restored.raise_house_tags(
        houses[0],
        Some(&rules),
        Some(&registry),
        FrameEffects::default(),
    );
    assert!(
        restored.trigger_runtime.globals_set.contains(&7),
        "the saved, nonlatching notification can refire"
    );
    assert_eq!(restored.trigger_runtime.trigger(live).unwrap().completed, 0);
}

#[test]
fn captured_house_event_uses_engineer_notification_and_keeps_the_original_raise_gate() {
    let (mut sim, rules, registry, houses) = event_ingress_fixture_with_rules(ENTRY_RULES);
    let building = sim
        .spawn_object("DEPOT", "House0", 16, 16, 0, &rules)
        .unwrap();
    let engineer = sim
        .spawn_object("ENGINEER", "House1", 17, 16, 0, &rules)
        .unwrap();
    let (tag, live) = install(&mut sim, &rules, 3, 0);
    sim.mission_assign_exact(
        engineer,
        crate::sim::mission::MissionId::from_known(
            crate::rules::mission_data::MissionType::Capture,
        ),
        0,
    )
    .unwrap();
    sim.substrate
        .entities
        .get_mut(engineer)
        .unwrap()
        .navigation
        .set_nav_com_for_test(Some(crate::sim::components::NavTargetRef::Object {
            id: building,
        }));
    // Declare an admitted paid arrival at the receiver, using the shared
    // Foot coordinate/occupation writer. Walking itself retains the existing
    // whole original Engineer-arrival corpus in world tests.
    sim.foot_set_location_marked(
        engineer,
        crate::sim::components::DriveCoord {
            x: 16 * 256 + 128,
            y: 16 * 256 + 128,
            z: 0,
        },
        Some(&rules),
        Some(&registry),
        FrameEffects::default(),
    );
    assert!(!sim.houses[&houses[0]].building_capture_notified());
    sim.infantry_per_cell_building_entry(
        engineer,
        &rules,
        Some(&registry),
        FrameEffects::default(),
    )
    .unwrap();
    assert_eq!(
        sim.substrate.entities.get(building).unwrap().owner(),
        houses[1]
    );
    assert!(sim.houses[&houses[0]].building_capture_notified());
    assert!(!sim.houses[&houses[1]].building_capture_notified());

    // 71EC9A requires3 even with House244=true. The existing ordinary
    // House poll8 must remain unable to satisfy it; do not invent a producer.
    sim.session.set_binary_frame_for_test(8);
    sim.raise_house_tags(
        houses[0],
        Some(&rules),
        Some(&registry),
        FrameEffects::default(),
    );
    assert!(!sim.trigger_runtime.globals_set.contains(&7));
    let mut persistence = false;
    let rng = sim.rng_state();
    assert!(sim.has_occurred(
        0,
        TagEventInput {
            kind: 3,
            ..Default::default()
        },
        Some(houses[1]),
        &mut persistence,
        Some(&rules),
        live
    ));
    assert!(!persistence);
    let mut effects = Vec::new();
    assert!(sim.dispatch_live_tag_event(
        tag,
        TagEventInput {
            kind: 3,
            ..Default::default()
        },
        &mut effects,
        Some(&rules),
        Some(&registry),
        FrameEffects::default()
    ));
    assert!(sim.trigger_runtime.globals_set.contains(&7));
    assert_eq!(sim.rng_state(), rng);

    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "Capture notification", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    restored.restore_after_snapshot_load().unwrap();
    assert!(restored.houses[&houses[0]].building_capture_notified());
    let mut persistence = false;
    assert!(restored.has_occurred(
        0,
        TagEventInput {
            kind: 3,
            ..Default::default()
        },
        None,
        &mut persistence,
        Some(&rules),
        live
    ));
}
