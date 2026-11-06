//! Tactical6D4764..6D478E -> Foot4DC340 through the production line builder.

use super::*;

fn view() -> TacticalViewport {
    TacticalViewport {
        camera: [-2000, -2000],
        clip: [0, 0, 4000, 4000],
        zoom: 1.,
    }
}

fn lines(sim: &Simulation, rules: &RuleSet, planning: bool, time: u32) -> Vec<SpriteInstance> {
    build_target_line_instances(
        &TargetLineState::default(),
        planning,
        Some(sim),
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
/// dashed walk, whose stores carry the A-buffer gate the GPU applies
/// (`FX_SURFACE_A_NONZERO`). With bit 9 of the clock
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
    let spans = |instances: &[SpriteInstance]| {
        instances
            .iter()
            .map(|span| (span.position, span.size, span.tint))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        spans(&lines(&sim, &rules, true, 0x200)),
        spans(&lit),
        "planning"
    );
    // Shroud leaves only the two boxes: they store everywhere, the dashes
    // only where A is nonzero.
    let (boxes, dashes): (Vec<&SpriteInstance>, Vec<_>) =
        lit.iter().partition(|span| span.draw_state.fx_flags == 0);
    assert_eq!(boxes.len(), 2);
    assert!(boxes.iter().all(|span| span.size == [3., 3.]));
    assert!(!dashes.is_empty());
    assert!(
        dashes
            .iter()
            .all(|span| span.draw_state.fx_flags == crate::render::draw_state::FX_SURFACE_A_NONZERO)
    );
    let box_tint = boxes[0].tint;
    assert!(lit.iter().all(|span| span.tint == box_tint));
    let pulsed = lines(&sim, &rules, false, 0x80);
    assert!(pulsed.iter().any(|span| span.tint != box_tint));

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
