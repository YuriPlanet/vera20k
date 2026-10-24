//! The tail of `InfantryClass::Per_Cell_Process` after its C4 arm
//! (`0x0051A7F8..0x0051A8C5`): a tethered arrival that fills its cell
//! scatters that cell; then an infantryman with nothing to do enters idle
//! mode, and whatever is queued commences.

use super::*;
use crate::sim::combat::TargetKind;
use crate::sim::components::DriveCoord;
use crate::sim::mission::MissionDispatchTimer;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::radio::{self, RadioPayload, RadioResponse};

const CELL: (u16, u16) = (12, 12);

/// Retail rules on flat ground, with a human Americans House.
fn retail_world() -> Option<(Simulation, RuleSet)> {
    let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art()?;
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    let owner = sim.intern("Americans");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, true, 0, 10,
        ));
    Some((sim, rules))
}

fn set_missions(sim: &mut Simulation, id: u64, current: MissionType, queued: MissionId) {
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .mission
        .apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(current),
            suspended: MissionId::NONE,
            queued,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
}

/// The arriver as the Walk leaves him at `0x0075BE3C`: still heading to the
/// spot he stands on, with the cell as his NavCom.
fn mid_arrival(sim: &mut Simulation, id: u64) {
    let entity = sim.substrate.entities.get_mut(id).unwrap();
    let at = position_world_coord(&entity.position);
    entity
        .navigation
        .set_nav_com_for_test(Some(NavTargetRef::cell(CELL.0, CELL.1)));
    entity
        .locomotor
        .as_mut()
        .unwrap()
        .set_walk_destination(Some(DriveCoord {
            x: at.x,
            y: at.y,
            z: at.z,
        }));
}

fn send(sim: &mut Simulation, rules: &RuleSet, from: u64, to: u64, message: RadioMessage) {
    assert_eq!(
        radio::transmit(
            sim,
            from,
            to,
            message,
            RadioPayload::default(),
            rules,
            FrameEffects::default(),
        ),
        RadioResponse::Roger
    );
}

fn arrive(sim: &mut Simulation, rules: &RuleSet, id: u64) {
    sim.per_cell_process(
        id,
        PerCellReason::Arrival,
        rules,
        None,
        FrameEffects::default(),
    )
    .unwrap();
}

/// `0x0051A84F..0x0051A886`: the third barracks product reaching a door cell
/// fills its three infantry spots (`& 0x1C`), and `Scatter_Objects(null, 1,
/// 1)` sends the two idle men there to nearby cells. The arriver is lifted
/// from the cell's list, as a Walk arrival leaves him, with his raw spot
/// still marked. Two men in the cell, or an arrival with no tether
/// (`+0x418`), scatter no one.
#[test]
fn a_tethered_arrival_that_fills_its_cell_scatters_the_men_there() {
    for case in ["full", "two_men", "untethered"] {
        let Some((mut sim, rules)) = retail_world() else {
            return;
        };
        let barracks = sim
            .spawn_object("GAPILE", "Americans", 4, 4, 0, &rules)
            .unwrap();
        let count = if case == "two_men" { 2 } else { 3 };
        let men: Vec<u64> = (0..count)
            .map(|_| {
                sim.spawn_object("E1", "Americans", CELL.0, CELL.1, 0, &rules)
                    .unwrap()
            })
            .collect();
        for &man in &men {
            let at = &sim.substrate.entities.get(man).unwrap().position;
            assert_eq!((at.rx(), at.ry()), CELL, "{case}");
        }
        let full = sim
            .substrate
            .raw_cell_occupation
            .ground_bits(CELL.0, CELL.1)
            & 0x1C
            == 0x1C;
        assert_eq!(full, count == 3, "{case}");
        let arriver = *men.last().unwrap();
        mid_arrival(&mut sim, arriver);
        sim.substrate.occupancy.remove(CELL.0, CELL.1, arriver);
        if case != "untethered" {
            send(&mut sim, &rules, arriver, barracks, RadioMessage::Hello);
            send(&mut sim, &rules, arriver, barracks, RadioMessage::Tether);
        }
        let rng = sim.scenario_rng.state();

        arrive(&mut sim, &rules, arriver);

        let arriver_entity = sim.substrate.entities.get(arriver).unwrap();
        assert_eq!(arriver_entity.tether.partner(), None, "{case}");
        assert_eq!(
            arriver_entity.navigation.nav_com(),
            Some(NavTargetRef::cell(CELL.0, CELL.1)),
            "{case}: the lifted arriver is not scattered"
        );
        for &man in &men[..men.len() - 1] {
            let nav_com = sim
                .substrate
                .entities
                .get(man)
                .unwrap()
                .navigation
                .nav_com();
            if case == "full" {
                assert!(
                    matches!(nav_com, Some(NavTargetRef::Cell { rx, ry }) if (rx, ry) != CELL),
                    "{case}: man {man} is sent to a nearby cell, not {nav_com:?}"
                );
            } else {
                assert_eq!(nav_com, None, "{case}");
            }
        }
        if case == "full" {
            assert_ne!(sim.scenario_rng.state(), rng);
        } else {
            assert_eq!(sim.scenario_rng.state(), rng, "{case}");
        }
    }
}

/// Per_Cell_Process51A812..51A84A plays the second VoiceComment after
/// radio8, even for a computer-owned man, without a Main selection draw.
#[test]
fn a_computer_owned_tethered_arrival_plays_the_second_comment() {
    let Some((mut sim, mut rules)) = retail_world() else {
        return;
    };
    let sounds = crate::rules::sound_ini::SoundRegistry::from_ini(
        &crate::rules::ini_parser::IniFile::from_str("[SoundList]\n0=First\n1=Second\n"),
    );
    rules.bind_type_sound_references(
        &crate::rules::ini_parser::IniFile::from_str("[E1]\nVoiceComment=First,Second\n"),
        &sounds,
    );
    let owner = sim.intern("Russians");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, false, 0, 10,
        ));
    assert!(!sim.house_is_human_player(owner));
    let barracks = sim
        .spawn_object("GAPILE", "Russians", 4, 4, 0, &rules)
        .unwrap();
    let man = sim
        .spawn_object("E1", "Russians", CELL.0, CELL.1, 0, &rules)
        .unwrap();
    mid_arrival(&mut sim, man);
    sim.substrate.occupancy.remove(CELL.0, CELL.1, man);
    send(&mut sim, &rules, man, barracks, RadioMessage::Hello);
    send(&mut sim, &rules, man, barracks, RadioMessage::Tether);
    sim.sound_events.clear_for_test();
    let before = sim.main_rng.state();
    arrive(&mut sim, &rules, man);
    let comments: Vec<_> = sim
        .sound_events
        .iter()
        .filter_map(|event| match event {
            SimSoundEvent::VocAt { sound_id, .. } => Some(sound_id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(comments, ["Second"]);
    assert_eq!(sim.main_rng.state(), before);
}

/// `0x0051A88B..0x0051A8C5`: an arrival with nothing queued, no NavCom, no
/// Target and no radio contact enters idle mode (Guard for a retail GI),
/// which Commence makes current at once; any one of those keeps his Move.
/// Commence asks no Ready_To_Commence: a queued Attack replaces a Sticky
/// mission, whose Ready answer is false.
#[test]
fn an_idle_arrival_guards_and_a_queued_mission_commences_at_once() {
    for case in ["idle", "nav_com", "target", "contact", "queued", "sticky"] {
        let Some((mut sim, rules)) = retail_world() else {
            return;
        };
        let barracks = sim
            .spawn_object("GAPILE", "Americans", 4, 4, 0, &rules)
            .unwrap();
        let man = sim
            .spawn_object("E1", "Americans", CELL.0, CELL.1, 0, &rules)
            .unwrap();
        let queued = if matches!(case, "queued" | "sticky") {
            MissionId::from_known(MissionType::Attack)
        } else {
            MissionId::NONE
        };
        let current = if case == "sticky" {
            MissionType::Sticky
        } else {
            MissionType::Move
        };
        set_missions(&mut sim, man, current, queued);
        match case {
            "nav_com" => mid_arrival(&mut sim, man),
            "target" | "queued" | "sticky" => {
                sim.substrate
                    .entities
                    .get_mut(man)
                    .unwrap()
                    .attack_target
                    .set_for_test(Some(TargetKind::Entity(barracks)));
            }
            "contact" => send(&mut sim, &rules, man, barracks, RadioMessage::Hello),
            _ => {}
        }
        if case == "sticky" {
            assert!(!sim.mission_ready_to_commence(man, &rules));
        }

        arrive(&mut sim, &rules, man);

        let mission = &sim.substrate.entities.get(man).unwrap().mission;
        let expected = match case {
            "idle" => MissionType::Guard,
            "queued" | "sticky" => MissionType::Attack,
            _ => MissionType::Move,
        };
        assert_eq!(mission.current(), MissionId::from_known(expected), "{case}");
        assert_eq!(mission.queued(), MissionId::NONE, "{case}");
    }
}

/// `0x0051A071..0x0051A0CE`: a Capture infantryman whose arrival finds no
/// building of his and who has no NavCom enters idle mode, and standing in
/// a building's cell takes `Scatter(null, 1, 0)` there and then; his tail
/// commences the Guard. Off a building he only takes the Guard.
#[test]
fn a_capturer_with_no_destination_idles_and_scatters_off_a_building() {
    for on_building in [true, false] {
        let Some((mut sim, rules)) = retail_world() else {
            return;
        };
        sim.spawn_object("GAPILE", "Americans", 4, 4, 0, &rules)
            .unwrap();
        let cell = if on_building { (4, 4) } else { CELL };
        let man = sim
            .construct_object_limbo_at_height(
                "E1",
                "Americans",
                cell.0,
                cell.1,
                0,
                0,
                &rules,
                FrameEffects::default(),
            )
            .unwrap();
        sim.substrate
            .entities
            .get_mut(man)
            .unwrap()
            .position
            .put_location(DriveCoord::cell(cell.0, cell.1, 0));
        assert!(matches!(
            sim.reveal_entity_with_rules(man, &rules),
            crate::sim::world::RevealOutcome::Revealed { .. }
        ));
        set_missions(&mut sim, man, MissionType::Capture, MissionId::NONE);
        assert_eq!(
            sim.substrate
                .entities
                .get(man)
                .unwrap()
                .navigation
                .nav_com(),
            None
        );

        arrive(&mut sim, &rules, man);

        let entity = sim.substrate.entities.get(man).unwrap();
        assert_eq!(
            entity.mission.current(),
            MissionId::from_known(MissionType::Guard),
            "on_building={on_building}"
        );
        let nav_com = entity.navigation.nav_com();
        if on_building {
            assert!(
                matches!(nav_com, Some(NavTargetRef::Cell { rx, ry }) if (rx, ry) != cell),
                "scattered off the building, not {nav_com:?}"
            );
        } else {
            assert_eq!(nav_com, None);
        }
    }
}
