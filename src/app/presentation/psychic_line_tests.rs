//! Tactical6D4764..6D478E -> Foot4DC340 through the production line builder.

use super::*;

fn view() -> TacticalViewport {
    TacticalViewport {
        camera: [-2000, -2000],
        clip: [0, 0, 4000, 4000],
        zoom: 1.,
    }
}

fn lines(
    sim: &Simulation,
    rules: &RuleSet,
    planning: bool,
    time: u32,
    alpha: u8,
) -> Vec<SpriteInstance> {
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
        &mut |_| alpha,
    )
}

/// An enemy tank aiming inside the local player's Psychic Sensor radius
/// draws its line, in planning mode too: two 3x3 owner-colour boxes and the
/// dashed walk, whose stores the A-buffer gates. With bit 9 of the clock
/// set the dash keeps the owner colour; otherwise it pulses toward white.
/// An undetected tank draws none, nor one whose owner IsHumanPlayer
/// (50B6F0) admits though the sensor still senses it.
#[test]
fn an_enemy_tank_sensed_by_a_psychic_sensor_draws_its_detection_line() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art() else {
        return;
    };
    let (mut sim, tank, _) = crate::sim::psychic_detection::psychic_sensor_watch_fixture(&rules);
    let lit = lines(&sim, &rules, false, 0x200, 127);
    let spans = |instances: &[SpriteInstance]| {
        instances
            .iter()
            .map(|span| (span.position, span.size, span.tint))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        spans(&lines(&sim, &rules, true, 0x200, 127)),
        spans(&lit),
        "planning"
    );
    let shrouded = lines(&sim, &rules, false, 0x200, 0);
    // Shroud leaves only the two boxes.
    assert_eq!(shrouded.len(), 2);
    assert!(shrouded.iter().all(|span| span.size == [3., 3.]));
    assert!(lit.len() > shrouded.len());
    let box_tint = shrouded[0].tint;
    assert!(lit.iter().all(|span| span.tint == box_tint));
    let pulsed = lines(&sim, &rules, false, 0x80, 127);
    assert!(pulsed.iter().any(|span| span.tint != box_tint));

    // Mode zero: a human-controlled owner other than the local player.
    let russians = sim.interner.get("Russians").unwrap();
    let house = sim.houses.get_mut(&russians).unwrap();
    house.set_is_human_for_test(true);
    house.set_player_control_for_test(true);
    assert!(!sim.session.game_mode_nonzero() && sim.house_is_human_player(russians));
    assert!(
        lines(&sim, &rules, false, 0x200, 127).is_empty(),
        "human owner"
    );
    let house = sim.houses.get_mut(&russians).unwrap();
    house.set_is_human_for_test(false);
    house.set_player_control_for_test(false);

    sim.substrate
        .entities
        .get_mut(tank)
        .unwrap()
        .attack_target
        .set_for_test(Some(crate::sim::combat::TargetKind::Cell(30, 30)));
    assert!(
        lines(&sim, &rules, false, 0x200, 127).is_empty(),
        "out of range"
    );
}
