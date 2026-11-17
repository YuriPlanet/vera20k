//! Tactical6D4764..6D478E -> Foot4DC340 through the production line builder.

use super::*;

fn view() -> TacticalViewport {
    TacticalViewport {
        camera: [-2000, -2000],
        clip: [0, 0, 4000, 4000],
    }
}

fn lines(sim: &Simulation, rules: &RuleSet, planning: bool, time: u32) -> Vec<SurfaceStroke> {
    build_target_line_strokes(
        &TargetLineState::default(),
        planning,
        Some(sim),
        Some(&crate::app::presentation::frame::PresentationFrame::of_test(sim, rules)),
        Some(rules),
        None,
        &HouseColorMap::new(),
        time,
        view(),
        None,
    )
}

/// An enemy tank aiming inside the local player's Psychic Sensor radius
/// draws its line, in planning mode too: two 3x3 owner-colour boxes and the
/// dashed walk, whose stroke carries the A-buffer gate the GPU applies
/// (`StoreGate::ANonzero`). With bit 9 of the clock
/// set the dash keeps the owner colour; otherwise it pulses toward white.
/// An undetected tank draws none, nor one whose owner IsHumanPlayer
/// (50B6F0) admits though the sensor still senses it.
#[test]
fn an_enemy_tank_sensed_by_a_psychic_sensor_draws_its_detection_line() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art() else {
        return;
    };
    let (mut sim, tank, _) = crate::sim::psychic_detection::psychic_sensor_watch_fixture(&rules);
    let lit = lines(&sim, &rules, false, 0x200);
    assert_eq!(lines(&sim, &rules, true, 0x200), lit, "planning");
    // Shroud leaves only the two boxes: they store everywhere, the dashes
    // only where A is nonzero.
    let (boxes, dashes): (Vec<&SurfaceStroke>, Vec<_>) = lit
        .iter()
        .partition(|stroke| stroke.gate() == StoreGate::All);
    assert_eq!(boxes.len(), 2);
    assert!(boxes.iter().all(|stroke| stroke.pixel_count() == 9));
    assert_eq!(dashes.len(), 1);
    assert!(
        dashes
            .iter()
            .all(|stroke| stroke.gate() == StoreGate::ANonzero)
    );
    let word = boxes[0].word();
    assert!(lit.iter().all(|stroke| stroke.word() == word));
    let pulsed = lines(&sim, &rules, false, 0x80);
    assert!(pulsed.iter().any(|stroke| stroke.word() != word));

    // Mode zero: a human-controlled owner other than the local player.
    let russians = sim.interner().get("Russians").unwrap();
    let house = sim.houses_mut_for_test().get_mut(&russians).unwrap();
    house.set_is_human_for_test(true);
    house.set_player_control_for_test(true);
    assert!(!sim.session().game_mode_nonzero() && sim.house_is_human_player(russians));
    assert!(lines(&sim, &rules, false, 0x200).is_empty(), "human owner");
    let house = sim.houses_mut_for_test().get_mut(&russians).unwrap();
    house.set_is_human_for_test(false);
    house.set_player_control_for_test(false);

    sim.entities_mut_for_test()
        .get_mut(tank)
        .unwrap()
        .attack_target
        .set_for_test(Some(crate::sim::combat::TargetKind::Cell(30, 30)));
    assert!(lines(&sim, &rules, false, 0x200).is_empty(), "out of range");
}
