//! World-side gates for the combat-produced EVA inputs:
//! `TechnoClass::Death_Announcement @ 0x004D98C0` (owner gate, radar type 7
//! request) and `HouseClass::NotifyUnderAttack @ 0x004F93E0` (own line, ally
//! line). Radar admission and its dedupe belong to the local client's event
//! array (`render::radar_events`); the world only publishes the request.

use super::{SimSoundEvent, Simulation};
use crate::sim::combat::{UnderAttackEvent, UnitLostEvent};
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::radar::{RadarEventRequest, RadarEventType};

fn house(sim: &mut Simulation, name: &str, human: bool) -> InternedId {
    let id = sim.interner.intern(name);
    sim.houses
        .insert(id, HouseState::new(id, 0, Some(id), human, 5_000, 10));
    sim.session.house_order.push(id);
    id
}

fn ally(sim: &mut Simulation, asker: &str, other: &str) {
    sim.house_alliances
        .entry(asker.to_ascii_uppercase())
        .or_default()
        .insert(other.to_ascii_uppercase());
}

fn unit_lost_requests(sim: &mut Simulation) -> Vec<(InternedId, RadarEventRequest)> {
    sim.sound_events
        .drain(..)
        .filter_map(|event| match event {
            SimSoundEvent::UnitLost { owner, radar } => Some((owner, radar)),
            _ => None,
        })
        .collect()
}

fn ally_line_requests(sim: &mut Simulation) -> Vec<(InternedId, RadarEventRequest)> {
    sim.sound_events
        .drain(..)
        .filter_map(|event| match event {
            SimSoundEvent::AllyUnderAttack { owner, radar } => Some((owner, radar)),
            _ => None,
        })
        .collect()
}

/// `0x004D98CA` owner gate, then the `CreateRadarEvent(7)` request
/// (`0x004D98FE`) at the victim's cell. The world applies no dedupe of its own.
#[test]
fn unit_lost_requests_radar_type_seven_for_human_owners_only() {
    let mut sim = Simulation::new();
    let human = house(&mut sim, "Americans", true);
    let ai = house(&mut sim, "Russians", false);

    sim.dispatch_unit_lost_events(&[UnitLostEvent {
        rx: 20,
        ry: 20,
        owner: ai,
    }]);
    assert!(
        unit_lost_requests(&mut sim).is_empty(),
        "AI deaths are silent"
    );

    sim.dispatch_unit_lost_events(&[
        UnitLostEvent {
            rx: 20,
            ry: 20,
            owner: human,
        },
        UnitLostEvent {
            rx: 25,
            ry: 20,
            owner: human,
        },
    ]);
    assert_eq!(
        unit_lost_requests(&mut sim),
        vec![
            (
                human,
                RadarEventRequest::new(RadarEventType::UnitLost, 20, 20)
            ),
            (
                human,
                RadarEventRequest::new(RadarEventType::UnitLost, 25, 20)
            ),
        ],
        "both deaths reach the client, whose event array decides the dedupe"
    );
}

/// The own line publishes type 3 (base) or type 4 (miner) at the victim cell
/// for every ping; the client's radar accept is the only rate limit.
#[test]
fn under_attack_own_line_requests_the_base_or_miner_radar_type() {
    let mut sim = Simulation::new();
    let human = house(&mut sim, "Americans", true);
    let ping = |rx: u16, miner: bool| UnderAttackEvent {
        rx,
        ry: 10,
        owner: human,
        miner,
        structure: !miner,
    };
    sim.dispatch_under_attack_events(&[ping(10, false), ping(12, false), ping(30, true)]);
    let requests: Vec<RadarEventRequest> = sim
        .sound_events
        .drain(..)
        .filter_map(|event| match event {
            SimSoundEvent::UnderAttack { radar, .. } => Some(radar),
            _ => None,
        })
        .collect();
    assert_eq!(
        requests,
        vec![
            RadarEventRequest::new(RadarEventType::BaseUnderAttack, 10, 10),
            RadarEventRequest::new(RadarEventType::BaseUnderAttack, 12, 10),
            RadarEventRequest::new(RadarEventType::HarvesterUnderAttack, 30, 10),
        ]
    );
}

/// `0x004F955D..0x004F95B3`: a building of a house that lists a human house
/// as its ally (one-way, the victim's own bitfield) gives that human the ally
/// line behind a `CreateRadarEvent(0x10)` request. Harvester pings (`UnitClass` path)
/// and `MultiplayPassive=` victims in a non-campaign mode do not.
#[test]
fn ally_under_attack_reaches_human_allies_of_the_victim_house() {
    let mut sim = Simulation::new();
    sim.session.game_mode_nonzero = true;
    let human = house(&mut sim, "Americans", true);
    let friend = house(&mut sim, "Russians", false);
    let stranger = house(&mut sim, "Germans", false);
    let other_human = house(&mut sim, "British", true);
    ally(&mut sim, "Russians", "Americans");
    ally(&mut sim, "Americans", "Germans");

    let ping = |owner: InternedId, rx: u16, structure: bool| UnderAttackEvent {
        rx,
        ry: 10,
        owner,
        miner: !structure,
        structure,
    };

    // The friend's building: the human hears the ally line; British does not
    // (Russians never listed them).
    sim.dispatch_under_attack_events(&[ping(friend, 10, true)]);
    assert_eq!(
        ally_line_requests(&mut sim),
        vec![(
            human,
            RadarEventRequest::new(RadarEventType::AllyUnderAttack, 10, 10)
        )]
    );
    let _ = other_human;

    // A one-way alliance from the human to the stranger is not enough: the
    // victim's own bitfield decides.
    sim.dispatch_under_attack_events(&[ping(stranger, 40, true)]);
    assert!(ally_line_requests(&mut sim).is_empty());

    // The friend's harvester: `UnitClass::ReceiveDamage` has no ally branch.
    sim.dispatch_under_attack_events(&[ping(friend, 60, false)]);
    assert!(ally_line_requests(&mut sim).is_empty());

    // A passive victim house (`HouseType+0x1A6`) is skipped in MP modes.
    sim.houses.get_mut(&friend).unwrap().multiplay_passive = true;
    sim.dispatch_under_attack_events(&[ping(friend, 90, true)]);
    assert!(ally_line_requests(&mut sim).is_empty());
    sim.session.game_mode_nonzero = false;
    sim.dispatch_under_attack_events(&[ping(friend, 120, true)]);
    assert_eq!(ally_line_requests(&mut sim).len(), 1);
}
