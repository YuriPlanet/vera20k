//! Script7/23 reach the existing House result, timer and ordinary-frame exit.

use super::*;
use crate::sim::house_state::HouseOutcomeKind;
use crate::sim::team_script_vm::packed_cell_tests::{frame, reload, scene};

#[test]
fn scripted_result_win_survives_restore_then_admits_normal_match_exit() {
    let Some((mut scene, team, member)) = scene("0=7,777", false, |_| {}) else {
        return;
    };
    let sim = scene.sim();
    let state = sim.team_script_vm.team(team).unwrap();
    assert!(state.refusal().is_none(), "{:?}", state.refusal());
    assert!(state.advance_pending());
    let player = sim.session.current_house().unwrap();
    assert_ne!(player, state.owner());
    assert!(sim.houses[&player].has_won());
    assert!(sim.houses[&state.owner()].outcome().is_none());
    let timer = sim.houses[&player].outcome_timer();
    assert_eq!(timer.duration(), 90, "retail SavourDelay=.1");
    assert!(sim.ready_outcome_for_owner(player).is_none());
    reload(&mut scene);
    assert_eq!(scene.sim().houses[&player].outcome_timer(), timer);
    for _ in 0..120 {
        frame(&mut scene);
        if scene.sim().ready_outcome_for_owner(player).is_some() {
            break;
        }
    }
    let sim = scene.sim();
    assert_eq!(
        sim.ready_outcome_for_owner(player).unwrap().kind,
        HouseOutcomeKind::Victory
    );
    assert!(
        sim.team_script_vm.team(team).is_none(),
        "EOF is not held on Script7"
    );
    assert!(sim.team_script_vm.team_for_member(member).is_none());
    assert!(sim.entities().get(member).unwrap().lifecycle.object_alive());
}

#[test]
fn scripted_result_loss_reaches_campaign_result_after_the_retail_delay() {
    let Some((mut runtime, member, ty)) = super::scout_tests::campaign_scene("0=23,-1") else {
        return;
    };
    let player = runtime.simulation.session.current_house().unwrap();
    let mut observed = None;
    let mut locked = false;
    for _ in 0..120 {
        let result = runtime
            .advance_frame(
                &[],
                22,
                crate::sim::world::TickLane::Ordinary,
                FrameEffects::default(),
            )
            .unwrap();
        locked |= result.trigger_effects.iter().any(|effect| {
            matches!(
                effect,
                crate::sim::trigger_runtime::TriggerEffect::InputLockChanged { locked: true, .. }
            )
        });
        let sim = &runtime.simulation;
        if let Some(team) = sim.team_script_vm.first_team_of_type(ty)
            && team.cursor() == 0
        {
            assert!(team.refusal().is_none(), "{:?}", team.refusal());
            assert!(team.advance_pending());
            assert_eq!(team.members().collect::<Vec<_>>(), [member]);
            assert_ne!(team.owner(), player);
            assert!(sim.houses[&team.owner()].outcome().is_none());
            observed = Some(team.id());
            break;
        }
    }
    let team = observed.expect("timed Trigger4 recruits the authored loss Team");
    assert!(locked, "native campaign FlagLose locks input synchronously");
    assert!(runtime.simulation.houses[&player].has_lost());
    assert_eq!(
        runtime.simulation.houses[&player]
            .outcome_timer()
            .duration(),
        90
    );
    assert!(runtime.simulation.ready_outcome_for_owner(player).is_none());
    for _ in 0..120 {
        runtime.advance_frame_for_tooling(&[], 22).unwrap();
        if runtime.simulation.ready_outcome_for_owner(player).is_some() {
            break;
        }
    }
    let sim = &runtime.simulation;
    assert_eq!(
        sim.ready_outcome_for_owner(player).unwrap().kind,
        HouseOutcomeKind::Defeat
    );
    assert!(sim.team_script_vm.team(team).is_none());
    assert!(sim.team_script_vm.team_for_member(member).is_none());
    assert!(sim.entities().get(member).unwrap().lifecycle.object_alive());
}
