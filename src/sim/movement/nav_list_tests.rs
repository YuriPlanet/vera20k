//! The Foot navigation list through its production callers, on retail
//! rules: Queue_Navigation_List's idle tail, Handle_Navigation_List from
//! both Enter_Idle receivers, Mission_Area_Guard's computer head, and the
//! list's clears and pointer expiry.

use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::components::NavTargetRef;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
use crate::sim::world::{FrameEffects, Simulation};

const A: NavTargetRef = NavTargetRef::Cell { rx: 14, ry: 10 };
const B: NavTargetRef = NavTargetRef::Cell { rx: 16, ry: 12 };
const ELSEWHERE: NavTargetRef = NavTargetRef::Cell { rx: 8, ry: 14 };

/// Retail rules on flat ground, with an Americans House that a human
/// controls or not.
fn retail_world(human: bool) -> Option<(Simulation, RuleSet)> {
    let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art()?;
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    let owner = sim.intern("Americans");
    sim.houses.insert(
        owner,
        crate::sim::house_state::HouseState::new(owner, 0, None, human, 0, 10),
    );
    Some((sim, rules))
}

fn set_mission(sim: &mut Simulation, id: u64, current: MissionType) {
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .mission
        .apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(current),
            suspended: MissionId::NONE,
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
}

fn list(sim: &Simulation, id: u64) -> (Vec<NavTargetRef>, bool) {
    let navigation = &sim.substrate.entities.get(id).unwrap().navigation;
    (navigation.nav_list().to_vec(), navigation.nav_list_loops())
}

fn nav_com(sim: &Simulation, id: u64) -> Option<NavTargetRef> {
    sim.substrate.entities.get(id).unwrap().navigation.nav_com()
}

/// `0x004DA0E0`: a guarding Foot with no NavCom and no WeaponsFactory
/// contact enters idle mode, whose Handle_Navigation_List sends it to the
/// post at once. A NavCom, another mission or a WeaponsFactory in radio
/// contact 0 leaves the post listed.
#[test]
fn a_queued_post_sends_an_idle_guard_there_at_once() {
    for case in ["guard", "nav_com", "move", "factory_contact"] {
        let Some((mut sim, rules)) = retail_world(true) else {
            return;
        };
        let e1 = sim
            .spawn_object("E1", "Americans", 10, 10, 0, &rules)
            .unwrap();
        set_mission(
            &mut sim,
            e1,
            if case == "move" {
                MissionType::Move
            } else {
                MissionType::Guard
            },
        );
        if case == "nav_com" {
            sim.substrate
                .entities
                .get_mut(e1)
                .unwrap()
                .navigation
                .set_nav_com_for_test(Some(ELSEWHERE));
        }
        if case == "factory_contact" {
            let factory = sim
                .spawn_object("GAWEAP", "Americans", 20, 20, 0, &rules)
                .unwrap();
            sim.substrate
                .entities
                .get_mut(e1)
                .unwrap()
                .radio_contacts
                .insert(factory);
        }
        sim.queue_navigation_list(e1, A, &rules, None, FrameEffects::default());
        if case == "guard" {
            assert_eq!(nav_com(&sim, e1), Some(A), "{case}");
            assert_eq!(list(&sim, e1), (vec![], false), "{case}");
        } else {
            assert_ne!(nav_com(&sim, e1), Some(A), "{case}");
            assert_eq!(list(&sim, e1), (vec![A], false), "{case}");
        }
    }
}

/// `0x004DA0F1..0x004DA157`: a Foot queueing itself on a list with entries
/// makes the list loop without joining it; any entry joining an empty list,
/// itself included, clears the loop first.
#[test]
fn queueing_itself_makes_the_list_loop() {
    let Some((mut sim, rules)) = retail_world(true) else {
        return;
    };
    let e1 = sim
        .spawn_object("E1", "Americans", 10, 10, 0, &rules)
        .unwrap();
    set_mission(&mut sim, e1, MissionType::Move);
    sim.queue_navigation_list(e1, A, &rules, None, FrameEffects::default());
    let itself = NavTargetRef::object(e1);
    sim.queue_navigation_list(e1, itself, &rules, None, FrameEffects::default());
    assert_eq!(list(&sim, e1), (vec![A], true));
    sim.queue_navigation_list(e1, B, &rules, None, FrameEffects::default());
    assert_eq!(list(&sim, e1), (vec![A, B], true));
    sim.substrate
        .entities
        .get_mut(e1)
        .unwrap()
        .navigation
        .set_nav_list_for_test(vec![], true);
    sim.queue_navigation_list(e1, B, &rules, None, FrameEffects::default());
    assert_eq!(list(&sim, e1), (vec![B], false));
    sim.substrate
        .entities
        .get_mut(e1)
        .unwrap()
        .navigation
        .set_nav_list_for_test(vec![], true);
    sim.queue_navigation_list(e1, itself, &rules, None, FrameEffects::default());
    assert_eq!(list(&sim, e1), (vec![itself], false));
}

/// Enter_Idle_Mode's Handle_Navigation_List (`0x004DA030`) on a Foot of
/// `type_name` listing A then B: the head becomes the NavCom and leaves the
/// list, and a looping list appends it again. A NavCom, a Wait mission or an
/// infantryman's TarCom keeps the list.
fn enter_idle_sends_the_list_head(type_name: &str, case: &str) {
    let Some((mut sim, rules)) = retail_world(true) else {
        return;
    };
    let id = sim
        .spawn_object(type_name, "Americans", 10, 10, 0, &rules)
        .unwrap();
    let mission = if case == "wait" {
        MissionType::Deliberate
    } else {
        MissionType::Move
    };
    set_mission(&mut sim, id, mission);
    let foot = sim.substrate.entities.get_mut(id).unwrap();
    foot.navigation
        .set_nav_list_for_test(vec![A, B], case == "loop");
    if case == "nav_com" {
        foot.navigation.set_nav_com_for_test(Some(ELSEWHERE));
    }
    if case == "tar_com" {
        foot.attack_target
            .set_for_test(Some(TargetKind::from(ELSEWHERE)));
    }
    if type_name == "MTNK" {
        sim.unit_enter_idle_mode(id, Some(&rules), false, FrameEffects::default());
    } else {
        sim.infantry_enter_idle_mode(id, &rules, None, FrameEffects::default());
    }
    let context = format!("{type_name} {case}");
    match case {
        "send" => {
            assert_eq!(nav_com(&sim, id), Some(A), "{context}");
            assert_eq!(list(&sim, id), (vec![B], false), "{context}");
        }
        "loop" => {
            assert_eq!(nav_com(&sim, id), Some(A), "{context}");
            assert_eq!(list(&sim, id), (vec![B, A], true), "{context}");
        }
        _ => assert_eq!(list(&sim, id), (vec![A, B], false), "{context}"),
    }
}

/// Unit Enter_Idle_Mode (`0x00738A2F`), after its Wait return
/// (`0x007389A8`).
#[test]
fn unit_enter_idle_sends_it_to_its_list_head() {
    for case in ["send", "loop", "nav_com", "wait"] {
        enter_idle_sends_the_list_head("MTNK", case);
    }
}

/// Infantry Enter_Idle_Mode (`0x0051CC21`), after its Wait return
/// (`0x0051CBD8`) and skipped with a TarCom.
#[test]
fn infantry_enter_idle_sends_him_to_his_list_head() {
    for case in ["send", "loop", "nav_com", "wait", "tar_com"] {
        enter_idle_sends_the_list_head("E1", case);
    }
}

/// `0x004D6B23..0x004D6B8D`: a computer Foot on Area Guard whose list head
/// is its archive is sent there before the mission body. A human's, or one
/// whose archive is not the head, keeps the list.
#[test]
fn a_computer_area_guard_takes_its_archived_list_head() {
    for (human, archive) in [(false, A), (true, A), (false, B)] {
        let Some((mut sim, rules)) = retail_world(human) else {
            return;
        };
        let id = sim
            .spawn_object("MTNK", "Americans", 10, 10, 0, &rules)
            .unwrap();
        set_mission(&mut sim, id, MissionType::AreaGuard);
        let foot = sim.substrate.entities.get_mut(id).unwrap();
        foot.navigation.set_nav_list_for_test(vec![A, B], false);
        foot.set_archive_target(Some(TargetKind::from(archive)));
        crate::sim::world::dispatch_foot_mission(
            &mut sim,
            id,
            &rules,
            crate::sim::world::ObjectAiCtx::default(),
        );
        let context = format!("human {human}, archive {archive:?}");
        if !human && archive == A {
            assert_eq!(nav_com(&sim, id), Some(A), "{context}");
            assert_eq!(list(&sim, id), (vec![B], false), "{context}");
        } else {
            assert_eq!(list(&sim, id), (vec![A, B], false), "{context}");
        }
    }
}

/// A tank listing another tank twice around a Cell, with the list looping.
fn listing_tank() -> Option<(Simulation, RuleSet, u64, u64)> {
    let (mut sim, rules) = retail_world(true)?;
    let id = sim
        .spawn_object("MTNK", "Americans", 10, 10, 0, &rules)
        .unwrap();
    let victim = sim
        .spawn_object("MTNK", "Americans", 20, 20, 0, &rules)
        .unwrap();
    let named = NavTargetRef::Object { id: victim };
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .navigation
        .set_nav_list_for_test(vec![named, A, named], true);
    Some((sim, rules, id, victim))
}

/// The MEGAMISSION event empties a Foot's list after its archive clear
/// (`0x004C7453`) and leaves the loop flag.
#[test]
fn a_move_order_empties_the_list() {
    let Some((mut sim, rules, id, _)) = listing_tank() else {
        return;
    };
    let order = crate::sim::command::Command::Move {
        entity_id: id,
        target_rx: 12,
        target_ry: 12,
    };
    assert!(sim.apply_command("Americans", &order, Some(&rules)));
    assert_eq!(list(&sim, id), (vec![], true));
}

/// A miner's HarvestCell order takes the same Foot clears.
#[test]
fn a_harvest_order_empties_the_list() {
    let Some((mut sim, rules)) = retail_world(true) else {
        return;
    };
    let id = sim
        .spawn_object("HARV", "Americans", 10, 10, 0, &rules)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .navigation
        .set_nav_list_for_test(vec![A, B], true);
    let order = crate::sim::command::Command::HarvestCell {
        entity_id: id,
        target_rx: 12,
        target_ry: 12,
    };
    assert!(sim.apply_command("Americans", &order, Some(&rules)));
    assert_eq!(list(&sim, id), (vec![], true));
}

/// The MEGAMISSION event's raw Area Guard arm (`0x004C7403..0x004C743F`)
/// keeps the list.
#[test]
fn an_area_guard_order_keeps_the_list() {
    let Some((mut sim, rules, id, victim)) = listing_tank() else {
        return;
    };
    let order = crate::sim::command::Command::Guard {
        entity_id: id,
        target: None,
    };
    assert!(sim.apply_command("Americans", &order, Some(&rules)));
    let named = NavTargetRef::Object { id: victim };
    assert_eq!(list(&sim, id), (vec![named, A, named], true));
}

/// The IDLE event empties a Foot's list (`0x004C758B`, `0x004C759C`) and
/// leaves the loop flag.
#[test]
fn a_stop_order_empties_the_list() {
    let Some((mut sim, rules, id, _)) = listing_tank() else {
        return;
    };
    let order = crate::sim::command::Command::Stop { entity_id: id };
    assert!(sim.apply_command("Americans", &order, Some(&rules)));
    assert_eq!(list(&sim, id), (vec![], true));
}

/// The Unit null destination's WeaponsFactory arm (`0x00742DFF`, before the
/// NavQueue at `0x00742E0E`): a Unit in radio contact with a war factory
/// keeps its NavCom and loses its list; without that contact the list stays.
#[test]
fn a_war_factory_null_destination_empties_the_list() {
    for factory_contact in [true, false] {
        let Some((mut sim, rules)) = retail_world(true) else {
            return;
        };
        let id = sim
            .spawn_object("MTNK", "Americans", 10, 10, 0, &rules)
            .unwrap();
        set_mission(&mut sim, id, MissionType::Move);
        if factory_contact {
            let factory = sim
                .spawn_object("GAWEAP", "Americans", 20, 20, 0, &rules)
                .unwrap();
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .radio_contacts
                .insert(factory);
        }
        let tank = sim.substrate.entities.get_mut(id).unwrap();
        tank.navigation.set_nav_com_for_test(Some(ELSEWHERE));
        tank.navigation.set_nav_list_for_test(vec![A, B], true);
        sim.set_unit_null_destination(id, Some(&rules), None, FrameEffects::default());
        if factory_contact {
            assert_eq!(nav_com(&sim, id), Some(ELSEWHERE));
            assert_eq!(list(&sim, id), (vec![], true));
        } else {
            assert_eq!(list(&sim, id), (vec![A, B], true));
        }
    }
}

/// Unit Assign_Destination's WeaponsFactory arm (`0x00742D46..0x00742E41`):
/// a tank still leaving its war factory keeps its NavCom, and an order waits
/// on its list for its next idle. The factory's own exit Cell (GAWEAP at
/// (20, 20): (24, 21)), an Enter mission or no factory in radio contact 0
/// take the order at once. A tank a Magnetron holds sends the factory
/// OVER_OUT and goes on without listing it.
#[test]
fn an_order_during_a_war_factory_exit_waits_on_the_list() {
    for case in ["exit", "exit_cell", "enter", "no_contact", "magnetron"] {
        let Some((mut sim, rules)) = retail_world(true) else {
            return;
        };
        let factory = sim
            .spawn_object("GAWEAP", "Americans", 20, 20, 0, &rules)
            .unwrap();
        let tank = sim
            .spawn_object("MTNK", "Americans", 10, 10, 0, &rules)
            .unwrap();
        let mission = if case == "enter" {
            MissionType::Enter
        } else {
            MissionType::Move
        };
        set_mission(&mut sim, tank, mission);
        let unit = sim.substrate.entities.get_mut(tank).unwrap();
        unit.navigation.set_nav_com_for_test(Some(ELSEWHERE));
        unit.navigation.set_nav_list_for_test(vec![B], false);
        if case != "no_contact" {
            unit.radio_contacts.insert(factory);
        }
        if case == "magnetron" {
            unit.lift.set_source_for_test(Some(factory + 1));
        }
        let order = if case == "exit_cell" {
            NavTargetRef::Cell { rx: 24, ry: 21 }
        } else {
            A
        };
        crate::sim::radio::take_transmit_log();
        sim.assign_destination_represented(
            tank,
            Some(order),
            Some(&rules),
            None,
            FrameEffects::default(),
        )
        .unwrap();
        let over_out = crate::sim::radio::take_transmit_log()
            .iter()
            .any(|send| send.sender_sid == tank && send.target_sid == factory && send.msg == 3);
        assert_eq!(over_out, case == "magnetron", "{case}");
        if case == "exit" {
            assert_eq!(nav_com(&sim, tank), Some(ELSEWHERE), "{case}");
            assert_eq!(list(&sim, tank), (vec![A], false), "{case}");
        } else {
            if case != "magnetron" {
                assert_eq!(nav_com(&sim, tank), Some(order), "{case}");
            }
            assert_eq!(list(&sim, tank), (vec![B], false), "{case}");
        }
    }
}

/// An order given while the factory door opens, with the tank inside on
/// Guard and no NavCom, is listed. Readiness then commences its queued Move
/// and Mission_Move's idle entry runs Unit Enter_Idle_Mode. At its Cell's
/// centre at height 0 the tank breaks the factory link (OVER_OUT,
/// `0x007389C4`) and Handle_Navigation_List sends it at once. Off the
/// centre, Handle's send re-enters the WeaponsFactory arm, which lists the
/// order again, and Handle's RemoveIndex (`0x004DA068`) drops it.
#[test]
fn an_order_before_the_factory_door_opens_goes_at_once_or_is_dropped() {
    for at_centre in [true, false] {
        let Some((mut sim, rules)) = retail_world(true) else {
            return;
        };
        let factory = sim
            .spawn_object("GAWEAP", "Americans", 20, 20, 0, &rules)
            .unwrap();
        let tank = sim
            .spawn_object("MTNK", "Americans", 10, 10, 0, &rules)
            .unwrap();
        set_mission(&mut sim, tank, MissionType::Guard);
        let unit = sim.substrate.entities.get_mut(tank).unwrap();
        unit.radio_contacts.insert(factory);
        if !at_centre {
            unit.position
                .put_location(crate::sim::components::DriveCoord {
                    x: 10 * 256 + 129,
                    y: 10 * 256 + 128,
                    z: 0,
                });
        }
        sim.assign_destination_represented(
            tank,
            Some(A),
            Some(&rules),
            None,
            FrameEffects::default(),
        )
        .unwrap();
        assert_eq!(list(&sim, tank), (vec![A], false), "listed");
        assert_eq!(nav_com(&sim, tank), None, "listed");
        set_mission(&mut sim, tank, MissionType::Move);
        sim.unit_enter_idle_mode(tank, Some(&rules), false, FrameEffects::default());
        assert_eq!(list(&sim, tank), (vec![], false), "at centre {at_centre}");
        let linked = sim
            .substrate
            .entities
            .get(tank)
            .unwrap()
            .radio_contacts
            .contains(factory);
        if at_centre {
            assert_eq!(nav_com(&sim, tank), Some(A));
            assert!(!linked);
        } else {
            assert_eq!(nav_com(&sim, tank), None);
            assert!(linked);
        }
    }
}

/// PointerExpired removes each entry naming the expiring object
/// (`0x004D9B43..0x004D9B95`), and the expiry broadcast reaches a Foot that
/// names it only there.
#[test]
fn pointer_expiry_prunes_the_list() {
    let Some((mut sim, rules, id, victim)) = listing_tank() else {
        return;
    };
    sim.detach_all_pointer_expired(victim, &rules, None, FrameEffects::default());
    assert_eq!(list(&sim, id), (vec![A], true));
}
