//! Civilian Aircraft419300 through the ordinary Enter/Scatter/arrival route.

use super::{RadioMessage, RadioResponse, take_transmit_log};
use crate::rules::ini_parser::IniFile;
use crate::sim::combat::TargetKind;
use crate::sim::command::Command;
use crate::sim::components::NavTargetRef;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::{EntryBaseAction, EntryClickAction, FrameEffects, Simulation};

/// Infantry51D455 can retarget the paid Walk head without dropping the Enter
/// mission or aircraft contact. PerCell51A29E then sends DOCK_NOW through
/// that contact when the head arrives at a Building. An ordinary Foot-speed
/// FNPC cannot pick an occupied gate; native Winged4834A7 can.
#[test]
fn civilian_scatter_into_gate_requests_aircraft_retreat_without_boarding() {
    let patch = IniFile::from_str(
        "[CIV1]\nCivilian=yes\nSelectable=yes\nFraidycat=yes\nSpeedType=Winged\nSpeed=1\n\
         [ORCA]\nPassengers=1\nLandable=yes\nAirportBound=no\nPrimary=none\nSecondary=none\n\
         [CARGOPLANE]\nDeployTime=.02\n\
         [GAGATE_A]\nFoundation=1x1\nDeployTime=0\nGateCloseDelay=100\n\
         [Enter]\nScatter=yes\n[CombatDamage]\nPlayerScatter=yes\n",
    );
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(&patch)
    else {
        return;
    };
    let rules = &retail.rules;
    let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
        &retail.processed_rules,
        Some(&retail.fixed_art),
    );
    for survivor in [false, true] {
        let mut sim = crate::sim::arena_fixture::rival_arena(rules, 31);
        sim.set_input_delay_ticks(0);
        let owner = sim.interner.get("Americans").unwrap();
        sim.session.set_current_house_for_test(Some(owner));
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        sim.session.game_options_mut_for_test().short_game = false;
        sim.intern_rule_type_ids(rules);
        sim.resolve_type_handles(rules);
        let gate = sim
            .spawn_object_with_overlay_registry(
                "GAGATE_A",
                "Americans",
                15,
                15,
                0,
                rules,
                &registry,
            )
            .expect("gate");
        let plane = sim
            .spawn_object_with_overlay_registry("ORCA", "Americans", 18, 15, 0, rules, &registry)
            .expect("aircraft");
        let man = sim
            .spawn_object_with_overlay_registry("CIV1", "Americans", 13, 15, 0, rules, &registry)
            .expect("civilian");
        if survivor {
            sim.substrate
                .entities
                .get_mut(man)
                .unwrap()
                .mark_nominal_survivor_for_test();
        }
        // A real Gate owner opens it and retains its hold. The transport
        // contact, mission, navigation and paid head below have no fixture writes.
        crate::sim::gate_runtime::request_gate_open_for_test(&mut sim, (15, 15), man, rules);
        for _ in 0..30 {
            if sim.substrate.entities.get(gate).unwrap().is_open_gate() {
                break;
            }
            sim.advance_tick(&[], Some(rules), None, Some(&registry), 67);
        }
        assert!(sim.substrate.entities.get(gate).unwrap().is_open_gate());
        assert_eq!(
            sim.entry_click_action(man, plane, EntryBaseAction::Select, rules),
            Some(EntryClickAction::Enter)
        );
        assert!(sim.apply_command(
            "Americans",
            &Command::clicked_mission(man, MissionType::Enter, TargetKind::Entity(plane),),
            Some(rules),
        ));
        let head_at_gate = |sim: &Simulation| {
            let actor = sim.substrate.entities.get(man).unwrap();
            actor
                .locomotor
                .as_ref()
                .and_then(|loco| loco.step_head())
                .filter(|head| head.x / 256 == 15 && head.y / 256 == 15)
        };
        for _ in 0..1200 {
            if head_at_gate(&sim).is_some() {
                break;
            }
            sim.advance_tick(&[], Some(rules), None, Some(&registry), 67);
        }
        let head = head_at_gate(&sim).expect("ordinary Enter commits a Walk head in the open gate");
        let actor = sim.substrate.entities.get(man).unwrap();
        assert_eq!(actor.mission.current().known(), Some(MissionType::Enter));
        assert_eq!(actor.radio_contacts.slot(0), Some(plane));
        assert_eq!(
            actor.navigation.nav_com(),
            Some(NavTargetRef::object(plane))
        );
        assert_ne!((actor.position.rx(), actor.position.ry()), (15, 15));

        let mut expected_rng = sim.scenario_rng.clone();
        expected_rng.next_range_i32_inclusive(0, 4);
        assert!(sim.apply_command(
            "Americans",
            &Command::Scatter { entity_id: man },
            Some(rules)
        ));
        assert_eq!(sim.scenario_rng.state(), expected_rng.state());
        let actor = sim.substrate.entities.get(man).unwrap();
        assert_eq!(actor.mission.current().known(), Some(MissionType::Enter));
        assert_eq!(actor.radio_contacts.slot(0), Some(plane));
        assert_eq!(actor.locomotor.as_ref().unwrap().step_head(), Some(head));
        assert_eq!(actor.navigation.nav_com(), Some(NavTargetRef::cell(15, 15)));
        take_transmit_log();
        let mut arrived = false;
        for _ in 0..600 {
            sim.advance_tick(&[], Some(rules), None, Some(&registry), 67);
            let sends = take_transmit_log();
            if let Some(send) = sends.iter().find(|send| {
                send.sender_sid == man
                    && send.target_sid == plane
                    && send.msg == RadioMessage::DockNow.code()
            }) {
                assert_eq!(send.reply, Some(RadioResponse::Code5.code()));
                arrived = true;
                break;
            }
        }
        assert!(
            arrived,
            "the real gate arrival sends DOCK_NOW to its retained aircraft contact"
        );
        let actor = sim.substrate.entities.get(man).unwrap();
        assert_eq!((actor.position.rx(), actor.position.ry()), (15, 15));
        assert!(!actor.in_limbo());
        assert!(actor.passenger_role.inside_transport().is_none());
        let aircraft = sim.substrate.entities.get(plane).unwrap();
        assert_eq!(aircraft.passenger_role.cargo().unwrap().count(), 0);
        if survivor {
            assert_ne!(
                aircraft.mission.queued(),
                MissionId::from_known(MissionType::Retreat)
            );
            assert_ne!(
                aircraft.mission.current().known(),
                Some(MissionType::Retreat)
            );
        } else {
            assert!(aircraft.attack_target.target().is_none());
            assert!(aircraft.navigation.nav_com().is_none());
            assert_eq!(
                aircraft.mission.queued(),
                MissionId::from_known(MissionType::Retreat)
            );
        }
    }

    // Native41930C closes at count == capacity, including an empty
    // nontransport. Null/noninfantry senders still get5 without a Retreat.
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 31);
    let plane = sim
        .spawn_object("CARGOPLANE", "Americans", 20, 20, 0, rules)
        .unwrap();
    let civilian = sim
        .spawn_object("CIV1", "Americans", 20, 21, 0, rules)
        .unwrap();
    assert_eq!(rules.object("CARGOPLANE").unwrap().passengers, 0);
    let now = sim.session.binary_frame();
    let effects = FrameEffects::default();
    let target = TargetKind::Cell(22, 20);
    let destination = NavTargetRef::cell(23, 20);
    sim.assign_target_represented(plane, Some(target), rules, Some(&registry), effects)
        .unwrap();
    sim.assign_aircraft_destination(plane, Some(destination), rules, effects);
    let queued = sim.substrate.entities.get(plane).unwrap().mission.queued();
    let dock = |sim: &mut Simulation, sender| {
        super::receive::receive_radio(
            sim,
            plane,
            sender,
            RadioMessage::DockNow,
            super::RadioPayload::default(),
            rules,
            effects,
        )
    };
    let door_ticks = rules.object("CARGOPLANE").unwrap().deploy_time_ticks as i32;
    let rng = sim.rng_state();
    for sender in [None, Some(plane)] {
        let aircraft = sim.substrate.entities.get_mut(plane).unwrap();
        aircraft.open_door(0, now);
        aircraft.advance_door(now);
        assert_eq!(dock(&mut sim, sender), RadioResponse::Code5);
        let aircraft = sim.substrate.entities.get(plane).unwrap();
        assert_eq!(aircraft.door_phase(), crate::sim::door::DoorPhase::Closing);
        assert_eq!(
            aircraft.door_timer_fields(),
            (now as i32, door_ticks, door_ticks)
        );
        assert_eq!(aircraft.attack_target.target(), Some(target));
        assert_eq!(aircraft.navigation.nav_com(), Some(destination));
        assert_eq!(aircraft.mission.queued(), queued);
    }
    assert_eq!(dock(&mut sim, Some(civilian)), RadioResponse::Code5);
    let aircraft = sim.substrate.entities.get(plane).unwrap();
    assert!(aircraft.attack_target.target().is_none());
    assert!(aircraft.navigation.nav_com().is_none());
    assert_eq!(
        aircraft.mission.queued(),
        MissionId::from_known(MissionType::Retreat)
    );
    // The early current-mission gate precedes door effects and even raw5.
    assert!(sim.substrate.mission_commence_exact(plane, now).unwrap());
    let aircraft = sim.substrate.entities.get_mut(plane).unwrap();
    aircraft.open_door(0, now);
    aircraft.advance_door(now);
    assert_eq!(dock(&mut sim, None), RadioResponse::None);
    let aircraft = sim.substrate.entities.get(plane).unwrap();
    assert_eq!(
        aircraft.door_phase(),
        crate::sim::door::DoorPhase::OpenStable
    );
    assert!(aircraft.attack_target.target().is_none());
    assert!(aircraft.navigation.nav_com().is_none());
    assert_eq!(sim.rng_state(), rng);
}
