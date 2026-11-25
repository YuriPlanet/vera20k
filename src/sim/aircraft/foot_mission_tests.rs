//! The Foot missions an aircraft shares with the ground classes: Capture and
//! Sabotage (`0x004D4B20`), Eaten (`0x004D4CB0`) and Rescue (`0x004DDF90`).
//!
//! The aircraft's own dispatch (`MissionClass::AI @ 0x005B3060`) runs the
//! shared Foot bodies, so its mission timer is rearmed instead of staying due.

use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::ObjectAiCtx;

/// The entry fixture with an Americans HORNET and both houses registered.
fn one_hornet() -> (
    crate::sim::world::Simulation,
    crate::rules::ruleset::RuleSet,
    crate::rules::overlay_types::OverlayTypeRegistry,
    u64,
) {
    let (mut sim, rules, registry) = crate::sim::world::entry_test_fixture::fixture_with_rules("");
    for (index, name) in ["Americans", "Russians"].into_iter().enumerate() {
        let house = sim.interner.intern(name);
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                house,
                index as i32,
                None,
                index == 0,
                0,
                10,
            ));
    }
    let plane = sim
        .spawn_object_with_overlay_registry("HORNET", "Americans", 15, 15, 0, &rules, &registry)
        .unwrap();
    (sim, rules, registry, plane)
}

/// An aircraft on Capture, Sabotage, Eaten or Rescue runs the shared Foot
/// handler and `MissionClass::AI` restarts its dispatch timer; before, the
/// timer stayed due and the aircraft only hovered.
#[test]
fn an_aircraft_mission_runs_its_foot_handler_and_rearms_the_timer() {
    for mission in [
        MissionType::Capture,
        MissionType::Sabotage,
        MissionType::Eaten,
        MissionType::Rescue,
    ] {
        let (mut sim, rules, registry, plane) = one_hornet();
        let entity = sim.substrate.entities.get_mut(plane).unwrap();
        // No destination and no Target. Rescue starts in state 1, where the
        // Foot body queues and commences AreaGuard at once.
        let initial_state = u32::from(mission == MissionType::Rescue);
        entity
            .mission
            .set_current_for_test(MissionId::from_known(mission), initial_state);
        assert!(entity.mission_handler_due(0));

        let mut ctx = ObjectAiCtx {
            overlay_registry: Some(&registry),
            ..ObjectAiCtx::default()
        };
        assert!(super::dispatch_mission(&mut sim, plane, &rules, &mut ctx).is_none());

        let entity = sim.substrate.entities.get(plane).unwrap();
        assert!(
            !entity.mission_handler_due(0),
            "{mission:?} must write the dispatch epilogue"
        );
        if mission == MissionType::Rescue {
            // The Foot body's status-1 arm queues AreaGuard and commences it.
            assert_eq!(
                entity.mission.current(),
                MissionId::from_known(MissionType::AreaGuard),
                "Rescue hands the aircraft to Area Guard"
            );
        }
    }
}

/// The shared body runs for the aircraft too: in state 0 with no Target it
/// takes the house-return arm, which sets the handler state and asks for the
/// house's home cell. The House500200 arm for an aircraft is not ported, so
/// the aircraft gets no destination (`sim::mission::rescue`).
#[test]
fn an_aircraft_on_rescue_runs_the_body_and_gets_no_home_cell() {
    let (mut sim, rules, registry, plane) = one_hornet();
    let entity = sim.substrate.entities.get_mut(plane).unwrap();
    entity
        .mission
        .set_current_for_test(MissionId::from_known(MissionType::Rescue), 0);
    assert!(entity.mission_handler_due(0));

    let mut ctx = ObjectAiCtx {
        overlay_registry: Some(&registry),
        ..ObjectAiCtx::default()
    };
    assert!(super::dispatch_mission(&mut sim, plane, &rules, &mut ctx).is_none());

    let entity = sim.substrate.entities.get(plane).unwrap();
    assert_eq!(
        entity.mission.handler_state(),
        1,
        "the shared body runs and takes the house-return arm"
    );
    assert_eq!(
        entity.navigation.nav_com(),
        None,
        "the aircraft has no house home cell"
    );
}
