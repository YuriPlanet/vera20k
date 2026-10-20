//! The firing blink of a `DisguiseWhenStill=` Unit (`UnitClass::Fire_At @
//! 0x0074143E..0x00741471`) and Evaluate_Candidate's disguise arm
//! (`0x006F84B1..0x006F854B`), through the production fire and scan owners on
//! retail rules.

use super::*;
use crate::sim::combat::threat_range::ScanMission;
use crate::sim::rng::SimRng;
use crate::sim::world::Simulation;

/// A Mirage of `mirage_house` disguised by its own UpdateDisguise, and a
/// Rhino of the other house two Cells away, on the rival arena (Russians a
/// computer house at Normal, Americans the human player).
fn scene(rules: &RuleSet, mirage_house: &str) -> (Simulation, u64, u64) {
    let rhino_house = if mirage_house == "Americans" {
        "Russians"
    } else {
        "Americans"
    };
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 1);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let mirage = sim
        .spawn_object("MGTK", mirage_house, 16, 16, 64, rules)
        .expect("mirage");
    let rhino = sim
        .spawn_object("HTNK", rhino_house, 18, 16, 64, rules)
        .expect("rhino");
    sim.session.set_binary_frame_for_test(100);
    sim.update_unit_disguise(mirage, rules).unwrap();
    let observer = sim.interner.get(rhino_house).unwrap();
    assert!(sim.substrate.entities.get(mirage).unwrap().is_disguised_to(
        observer,
        None,
        None,
        &sim.interner
    ));
    (sim, mirage, rhino)
}

fn blink(sim: &mut Simulation, mirage: u64, duration: i32) {
    let frame = sim.session.binary_frame();
    sim.substrate
        .entities
        .get_mut(mirage)
        .unwrap()
        .disguise
        .as_mut()
        .unwrap()
        .start_fire_blink(frame, duration);
}

/// Retail `[MirageGun] DisguiseFakeBlinkTime=15` (`MirageGunE` 5) and
/// `[General] DisabledDisguiseDetectionPercent=15,5,2` through the production
/// readers; a Mirage's launched shot starts its blink at the firing frame for
/// its weapon's time.
#[test]
fn a_retail_mirage_shot_starts_its_blink_for_its_weapons_fake_blink_time() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    assert_eq!(
        rules.weapon("MirageGun").unwrap().disguise_fake_blink_time,
        15
    );
    assert_eq!(
        rules.weapon("MirageGunE").unwrap().disguise_fake_blink_time,
        5
    );
    assert_eq!(
        rules.general.disabled_disguise_detection_percent,
        [15, 5, 2]
    );

    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 1);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let mirage = sim
        .spawn_object("MGTK", "Americans", 16, 16, 64, rules)
        .expect("mirage");
    let rhino = sim
        .spawn_object("HTNK", "Russians", 19, 16, 64, rules)
        .expect("rhino");
    assert!(sim.apply_command(
        "Americans",
        &crate::sim::command::Command::Attack {
            attacker_id: mirage,
            target_id: rhino,
        },
        Some(rules)
    ));
    let timer = |sim: &Simulation| {
        sim.substrate
            .entities
            .get(mirage)
            .unwrap()
            .disguise
            .as_ref()
            .unwrap()
            .blink_timer()
    };
    let mut fired = None;
    for _ in 0..90 {
        let frame = sim.session.binary_frame();
        sim.advance_tick(&[], Some(rules), None, None, 67);
        if timer(&sim).duration() != 0 {
            fired = Some(frame);
            break;
        }
    }
    let fired = fired.expect("the Mirage fires at the Rhino");
    let timer = timer(&sim);
    assert_eq!(timer.duration(), 15);
    assert!(
        (fired as i32..=sim.session.binary_frame() as i32).contains(&timer.start_frame()),
        "the blink starts in the firing frame"
    );
}

/// Without a running blink a disguised Mirage is refused without a draw. While
/// it blinks, a computer attacker draws once on the Scenario stream and takes
/// the Mirage when the draw is at most its Normal entry (5).
#[test]
fn a_computer_attacker_sees_a_blinking_mirage_only_on_its_detection_roll() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let pick = |sim: &mut Simulation, rhino| {
        sim.greatest_threat_represented(rules, None, rhino, ScanMission::Guard, None)
    };

    let (mut sim, _, rhino) = scene(rules, "Americans");
    let before = sim.scenario_rng.logical_state();
    assert_eq!(pick(&mut sim, rhino), None, "no blink: refused");
    assert_eq!(
        sim.scenario_rng.logical_state(),
        before,
        "no blink: no draw"
    );

    for (roll, admitted) in [(5, true), (6, false), (0, true), (99, false)] {
        let (mut sim, mirage, rhino) = scene(rules, "Americans");
        blink(&mut sim, mirage, 15);
        sim.scenario_rng = SimRng::answering(0, 99, roll);
        let mut once = sim.scenario_rng.clone();
        once.next_range_i32_inclusive(0, 99);
        assert_eq!(
            pick(&mut sim, rhino),
            admitted.then_some(mirage),
            "roll {roll}"
        );
        assert_eq!(
            sim.scenario_rng.logical_state(),
            once.logical_state(),
            "roll {roll}: exactly one draw"
        );
    }

    // The blink ends after its 15 frames.
    let (mut sim, mirage, rhino) = scene(rules, "Americans");
    blink(&mut sim, mirage, 15);
    sim.session.set_binary_frame_for_test(115);
    sim.scenario_rng = SimRng::answering(0, 99, 0);
    let before = sim.scenario_rng.logical_state();
    assert_eq!(pick(&mut sim, rhino), None);
    assert_eq!(sim.scenario_rng.logical_state(), before);
}

/// IsControlledByHuman (`0x0050B730`) of the attacking house: the human
/// player's tank never draws and never sees through the disguise.
#[test]
fn a_human_attacker_never_rolls_against_a_blinking_mirage() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, mirage, rhino) = scene(rules, "Russians");
    blink(&mut sim, mirage, 15);
    sim.scenario_rng = SimRng::answering(0, 99, 0);
    let before = sim.scenario_rng.logical_state();
    assert_eq!(
        sim.greatest_threat_represented(rules, None, rhino, ScanMission::Guard, None),
        None
    );
    assert_eq!(sim.scenario_rng.logical_state(), before);
}

/// A Hunt's flat walk over 256 or more objects is scored side by side
/// ([`flat_scores`]); the blinking Mirage's roll is drawn again on the shared
/// stream, so the pick and the stream equal the one-by-one walk's.
#[test]
fn a_side_by_side_hunt_walk_rolls_like_the_one_by_one_walk() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let run = |serial: bool| {
        let mut sim = crate::sim::arena_fixture::rival_arena_with_size(rules, 1, 64);
        sim.intern_rule_type_ids(rules);
        sim.resolve_type_handles(rules);
        let mirage = sim
            .spawn_object("MGTK", "Americans", 16, 16, 64, rules)
            .expect("mirage");
        let rhino = sim
            .spawn_object("HTNK", "Russians", 18, 16, 64, rules)
            .expect("rhino");
        for index in 0..300u16 {
            sim.spawn_object(
                "E1",
                "Americans",
                20 + index % 30,
                30 + index / 30,
                64,
                rules,
            )
            .expect("rifleman");
        }
        sim.session.set_binary_frame_for_test(100);
        sim.update_unit_disguise(mirage, rules).unwrap();
        blink(&mut sim, mirage, 15);
        sim.scenario_rng = SimRng::answering(0, 99, 0);
        SERIAL_FLAT_SCORES.set(serial);
        let walks = FLAT_WALKS_SIDE_BY_SIDE.get();
        let pick = sim.greatest_threat_represented(rules, None, rhino, ScanMission::Hunt, None);
        SERIAL_FLAT_SCORES.set(false);
        (
            pick,
            sim.scenario_rng.logical_state(),
            FLAT_WALKS_SIDE_BY_SIDE.get() != walks,
        )
    };
    let one_by_one = run(true);
    let side_by_side = run(false);
    assert!(!one_by_one.2 && side_by_side.2, "the walks took both paths");
    assert_eq!(side_by_side.0, one_by_one.0);
    assert_eq!(side_by_side.1, one_by_one.1);
    let mut once = SimRng::answering(0, 99, 0);
    once.next_range_i32_inclusive(0, 99);
    assert_eq!(one_by_one.1, once.logical_state(), "exactly one roll");
}
