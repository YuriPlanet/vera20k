//! Phobos TAction 610 `SetTeamDelay` through the authored trigger path.
use super::*;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;

fn run_team_delay_action(value: &str) -> (Simulation, RuleSet, [InternedId; 3]) {
    let (mut sim, rules, registry, houses) = super::tests::event_ingress_fixture();
    super::tests::install_event_ingress_map(
        &mut sim,
        &rules,
        &format!(
            "[Triggers]\nDELAY=Country0,<none>,Delay,0,1,1,1,0\n\
             [Events]\nDELAY=1,8,0,0\n\
             [Actions]\nDELAY=1,610,0,0,{value},0,0,0,A\n\
             [Tags]\nDELAYTAG=0,Delay,DELAY\n"
        ),
    );
    sim.session.set_binary_frame_for_test(10);
    // Native `SetTeamDelay` only shortens the current team timer, so leave a
    // long remainder for the 40-frame cap to choose.
    sim.houses
        .get_mut(&houses[0])
        .unwrap()
        .team_creation_mut()
        .restart(0, 100);
    let _ = sim.advance_triggers(&rules, Some(&registry), FrameEffects::default());
    (sim, rules, houses)
}

#[test]
fn set_team_delay_action_stores_the_fixed_timer_on_the_incoming_house() {
    let (sim, _rules, houses) = run_team_delay_action("40");
    let house = &sim.houses[&houses[0]];
    assert_eq!(house.team_creation().team_delay(), 40);
    assert_eq!(
        (
            house.team_creation().timer().start_frame(),
            house.team_creation().timer().duration()
        ),
        (10, 40),
        "native Timer.Start(40) at the action's frame"
    );
    // Country0 binds to the first registered House; the second Country0
    // instance and Country7 keep the constructor sentinel.
    for owner in [houses[1], houses[2]] {
        let house = &sim.houses[&owner];
        assert_eq!(house.team_creation().team_delay(), -1);
        assert_eq!(house.team_creation().timer(), CdTimer::started(0, 1));
    }
}

#[test]
fn set_team_delay_negative_value_restores_the_difficulty_row() {
    let (mut sim, mut rules, houses) = run_team_delay_action("40");
    rules.general.team_delays = vec![11, 22, 33];
    let house = sim.houses.get_mut(&houses[0]).unwrap();
    house.team_creation_mut().restart(20, 100);
    let stock = house.difficulty_value(&rules.general.team_delays);
    house
        .team_creation_mut()
        .set_team_delay_action(20, -1, stock);
    assert_eq!(house.team_creation().team_delay(), -1);
    let timer = house.team_creation().timer();
    assert_eq!(timer.start_frame(), 20);
    assert_eq!(
        timer.duration(),
        stock,
        "a negative Param3 caps the timer at the house's difficulty row"
    );
}

#[test]
fn set_team_delay_paused_timer_takes_the_shorter_remainder() {
    let mut creation = super::super::ai_team_creation::HouseTeamCreation::default();
    creation.set_team_timer_for_test(CdTimer::from_raw(crate::sim::timer::PAUSED_START_FRAME, 25));
    creation.set_team_delay_action(0, -1, 10);
    assert_eq!(creation.team_delay(), -1);
    assert_eq!(
        creation.timer(),
        CdTimer::from_raw(crate::sim::timer::PAUSED_START_FRAME, 10)
    );
}
