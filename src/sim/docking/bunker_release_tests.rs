//! Release-body regression checks derived from4593A0/4595C0 instructions.
//! Saved independent reads: .local/bunker-release-native.txt. These are Rust
//! integration tests, not native executable comparisons or exit-search parity.
use super::*;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::ground_pose::position_world_coord;
use crate::sim::radio::{Contacts, RadioTestEvent};
use crate::util::fixed_math::SimFixed;
use std::cell::RefCell;

#[derive(Debug)]
struct Boundary {
    name: &'static str,
    unit_link: Option<BunkerLink>,
    building_link: Option<u64>,
    powered: bool,
    speed: Option<SimFixed>,
    head: Option<DriveCoord>,
    destination: Option<DriveCoord>,
}

thread_local! {
    static TRACE: RefCell<Vec<Boundary>> = const { RefCell::new(Vec::new()) };
}

pub(super) fn record(sim: &Simulation, building: u64, unit: u64, name: &'static str) {
    let unit = sim.substrate.entities.get(unit);
    TRACE.with(|trace| {
        trace.borrow_mut().push(Boundary {
            name,
            unit_link: unit.map(|unit| unit.bunker_link),
            building_link: sim
                .substrate
                .entities
                .get(building)
                .and_then(|b| b.bunker_occupant),
            powered: unit
                .and_then(|unit| unit.locomotor.as_ref())
                .is_some_and(|loco| loco.is_powered()),
            speed: unit.map(|unit| unit.foot_speed.applied_fraction),
            head: unit
                .and_then(|unit| unit.drive_locomotion.as_ref())
                .and_then(|drive| drive.head_to),
            destination: unit
                .and_then(|unit| unit.drive_locomotion.as_ref())
                .and_then(|drive| drive.destination),
        })
    });
}

fn take_trace() -> Vec<Boundary> {
    TRACE.with(|trace| std::mem::take(&mut *trace.borrow_mut()))
}

fn release_fixture() -> Simulation {
    let mut sim = super::tests::installed_sim();
    let building = sim.substrate.entities.get_mut(2).unwrap();
    building.foundation = "2x3".to_owned();
    building.position.sub_x = SimFixed::from_num(64);
    building.position.sub_y = SimFixed::from_num(192);
    building.position.exact_z_leptons = Some(731);
    building.radio_contacts.insert(1);
    let unit = sim.substrate.entities.get_mut(1).unwrap();
    unit.radio_contacts.insert(2);
    unit.locomotor.as_mut().unwrap().power_off();
    unit.foot_speed.applied_fraction = SimFixed::lit("0.25");
    unit.foot_speed.cached_current_speed = 7;
    let drive = unit.drive_locomotion.as_mut().unwrap();
    drive.track.residual = 971;
    drive.track.reversed = true;
    drive.destination = Some(DriveCoord::cell(7, 8, 417));
    take_trace();
    crate::sim::radio::clear_test_trace();
    sim
}

fn release_head() -> DriveCoord {
    //447AC0 foundation-center addition, then459401/407 (or459726/72C).
    DriveCoord {
        x: 2624,
        y: 3136,
        z: 731,
    }
}

#[test]
fn sell_release_uses_building_center_preserves_pose_and_orders_links_after_speed() {
    let mut sim = release_fixture();
    let pose = position_world_coord(&sim.substrate.entities.get(1).unwrap().position);
    release_sell_destroy(&mut sim, 2);
    let trace = take_trace();
    assert_eq!(
        trace.iter().map(|b| b.name).collect::<Vec<_>>(),
        [
            "power",
            "force",
            "speed",
            "unit-link",
            "building-link",
            "break"
        ]
    );
    assert!(trace[0].powered);
    assert_eq!(trace[0].speed, Some(SimFixed::lit("0.25")));
    assert_eq!(trace[1].head, Some(release_head()));
    assert_eq!(trace[1].destination, Some(release_head()));
    assert_eq!(trace[1].speed, Some(SimFixed::lit("0.25")));
    assert_eq!(trace[2].speed, Some(SIM_ONE));
    assert_eq!(trace[2].unit_link, Some(BunkerLink::Installed(2)));
    assert_eq!(trace[2].building_link, Some(1));
    assert_eq!(trace[3].unit_link, Some(BunkerLink::None));
    assert_eq!(trace[3].building_link, Some(1));
    assert_eq!(trace[4].building_link, None);
    let unit = sim.substrate.entities.get(1).unwrap();
    assert_eq!(position_world_coord(&unit.position), pose);
    assert_eq!(unit.facing, 0);
    let drive = unit.drive_locomotion.as_ref().unwrap();
    assert_eq!(
        (
            drive.track.turn_index,
            drive.track.cursor,
            drive.track.residual
        ),
        (0x47, 0, 971)
    );
    assert!(drive.track.reversed);
    assert_eq!(unit.foot_speed.cached_current_speed, 7);
    assert!(!unit.lifecycle.in_limbo && unit.in_logic_vector);
    assert!(!unit.radio_contacts.contains(2));
    assert!(
        !sim.substrate
            .entities
            .get(2)
            .unwrap()
            .radio_contacts
            .contains(1)
    );
    assert_eq!(
        crate::sim::radio::take_test_trace(),
        vec![
            RadioTestEvent::SenderBreakCleared {
                sender_sid: 2,
                target_sid: 1
            },
            RadioTestEvent::ReceiverClassEffect {
                receiver_sid: 1,
                sender_sid: 2
            },
            RadioTestEvent::ReceiverCommonCleared {
                receiver_sid: 1,
                sender_sid: 2
            },
        ]
    );
}

#[test]
fn normal_release_clears_unit_first_and_assigns_destination_without_teleport() {
    let mut sim = release_fixture();
    let pose = position_world_coord(&sim.substrate.entities.get(1).unwrap().position);
    release_normal(&mut sim, 2, &super::tests::rules(), None);
    let trace = take_trace();
    assert_eq!(
        trace.iter().map(|b| b.name).collect::<Vec<_>>(),
        [
            "unit-link",
            "power",
            "force",
            "speed",
            "destination",
            "move",
            "building-link",
            "break"
        ]
    );
    assert_eq!(trace[0].unit_link, Some(BunkerLink::None));
    assert!(!trace[0].powered);
    assert_eq!(trace[2].head, Some(release_head()));
    assert_eq!(trace[2].destination, Some(release_head()));
    assert_eq!(trace[4].head, Some(release_head()));
    assert_eq!(trace[4].destination, Some(DriveCoord::cell(9, 11, 0)));
    assert_eq!(trace[5].building_link, Some(1));
    let unit = sim.substrate.entities.get(1).unwrap();
    assert_eq!(position_world_coord(&unit.position), pose);
    assert_eq!(unit.navigation.nav_com, Some(NavTargetRef::cell(9, 11)));
    assert_eq!(
        unit.mission.queued(),
        MissionId::from_known(MissionType::Move)
    );
    let building = sim.substrate.entities.get(2).unwrap();
    assert_eq!(
        building.mission.queued(),
        MissionId::from_known(MissionType::Guard)
    );
    assert_eq!(building.bunker_runtime.unwrap().state, BunkerState::Idle);
}

#[test]
fn force_limbo_early_return_does_not_skip_caller_speed_links_or_break() {
    let mut sim = release_fixture();
    // A supplied lifecycle edge into Force is separate from ordinary install,
    // which never conceals. Force's callback-specific tests live with its host.
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .lifecycle
        .in_limbo = true;
    release_sell_destroy(&mut sim, 2);
    let unit = sim.substrate.entities.get(1).unwrap();
    assert!(unit.lifecycle.in_limbo, "release must not resurrect/reveal");
    assert_eq!(unit.foot_speed.applied_fraction, SIM_ONE);
    assert_eq!(unit.bunker_link, BunkerLink::None);
    assert!(!unit.radio_contacts.contains(2));
    let drive = unit.drive_locomotion.as_ref().unwrap();
    assert_eq!(drive.head_to, None);
    assert!(!drive.track_valid);
    assert_eq!(drive.track.turn_index, 0x47);
    assert_eq!(drive.destination, Some(DriveCoord::cell(7, 8, 417)));
    assert_eq!(sim.substrate.entities.get(2).unwrap().bunker_occupant, None);
}

#[test]
fn break_targets_slot_zero_even_when_it_is_not_the_released_unit() {
    let mut sim = release_fixture();
    let mut contact = GameEntity::test_default(3, "TANK", "Americans", 14, 14);
    contact.radio_contacts.insert(2);
    sim.substrate.entities.insert(contact);
    let building = sim.substrate.entities.get_mut(2).unwrap();
    building.radio_contacts = Contacts::with_capacity(2);
    building.radio_contacts.insert(3);
    building.radio_contacts.insert(1);
    release_sell_destroy(&mut sim, 2);
    assert!(
        !sim.substrate
            .entities
            .get(3)
            .unwrap()
            .radio_contacts
            .contains(2)
    );
    assert!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .radio_contacts
            .contains(2)
    );
    assert!(
        sim.substrate
            .entities
            .get(2)
            .unwrap()
            .radio_contacts
            .contains(1)
    );
}

#[test]
fn empty_slot_zero_does_not_scan_later_contacts() {
    let mut sim = release_fixture();
    let building = sim.substrate.entities.get_mut(2).unwrap();
    building.radio_contacts = Contacts::with_capacity(2);
    building.radio_contacts.insert(3);
    building.radio_contacts.insert(1);
    building.radio_contacts.remove(3);
    release_sell_destroy(&mut sim, 2);
    assert!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .radio_contacts
            .contains(2)
    );
    assert!(
        sim.substrate
            .entities
            .get(2)
            .unwrap()
            .radio_contacts
            .contains(1)
    );
    assert!(crate::sim::radio::take_test_trace().is_empty());
}

#[test]
fn nonunit_link_does_not_release_or_reset_bunker() {
    for normal in [false, true] {
        let mut sim = release_fixture();
        sim.substrate.entities.get_mut(1).unwrap().category = EntityCategory::Infantry;
        if normal {
            release_normal(&mut sim, 2, &super::tests::rules(), None);
        } else {
            release_sell_destroy(&mut sim, 2);
        }
        assert!(take_trace().is_empty());
        let unit = sim.substrate.entities.get(1).unwrap();
        assert_eq!(unit.bunker_link, BunkerLink::Installed(2));
        assert!(!unit.locomotor.as_ref().unwrap().is_powered());
        assert_eq!(unit.foot_speed.applied_fraction, SimFixed::lit("0.25"));
        let building = sim.substrate.entities.get(2).unwrap();
        assert_eq!(building.bunker_occupant, Some(1));
        assert_eq!(
            building.bunker_runtime.unwrap().state,
            BunkerState::Occupied
        );
    }
}
