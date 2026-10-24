//! Script13 -> Infantry521C60 -> shared Do_Action and Doing/Stage completion.

use super::*;
use crate::sim::movement::infantry_action::{DO_GUARD, DO_IDLE1, DO_IDLE2, DO_READY};
use crate::sim::team_script_vm::packed_cell_tests::{frame, reload, scene};

fn doing(sim: &Simulation, member: u64) -> i32 {
    sim.entities()
        .get(member)
        .unwrap()
        .mission_leaf
        .as_infantry()
        .unwrap()
        .doing()
}

#[test]
fn recruited_infantry_plays_idle_two_across_reload_then_finishes_its_script() {
    let Some((mut scene, team, member)) = scene("0=5,20\n1=13,1\n2=5,20", false, |_| {}) else {
        return;
    };
    // Normal regrouping must finish before an idle request can remain visible;
    // a moving member's later Infantry movement action can replace its Doing.
    for _ in 0..400 {
        frame(&mut scene);
        if scene.sim().team_script_vm.team(team).unwrap().cursor() == 1 {
            break;
        }
    }
    let sim = scene.sim();
    let state = sim.team_script_vm.team(team).unwrap();
    assert_eq!(state.cursor(), 1);
    assert!(state.refusal().is_none(), "{:?}", state.refusal());
    assert!(state.advance_pending());
    assert_eq!(doing(sim, member), DO_IDLE2);
    let actor = sim.entities().get(member).unwrap();
    let stage = *actor.native_stage();
    let idle_timer = actor.infantry.as_ref().unwrap().idle_action_timer();
    reload(&mut scene);
    let actor = scene.sim().entities().get(member).unwrap();
    assert_eq!(*actor.native_stage(), stage);
    assert_eq!(
        actor.infantry.as_ref().unwrap().idle_action_timer(),
        idle_timer
    );
    assert_eq!(doing(scene.sim(), member), DO_IDLE2);
    let mut completed_animation = false;
    for _ in 0..400 {
        frame(&mut scene);
        let sim = scene.sim();
        if sim.team_script_vm.team(team).is_none() {
            break;
        }
        completed_animation |= matches!(doing(sim, member), DO_READY | DO_GUARD);
    }
    let sim = scene.sim();
    assert!(
        completed_animation,
        "Doing/Stage completes the requested idle"
    );
    assert!(
        sim.team_script_vm.team(team).is_none(),
        "Script13 reaches EOF"
    );
    assert!(sim.team_script_vm.team_for_member(member).is_none());
    assert!(sim.entities().get(member).unwrap().lifecycle.object_alive());
}

#[test]
fn idle_request_respects_action_refusal_and_other_foot_classes_are_empty_receivers() {
    use crate::sim::movement::infantry_action::DO_DOWN;

    let Some((mut scene, team, member)) = scene("0=13,1\n1=5,20", false, |_| {}) else {
        return;
    };
    frame(&mut scene);
    let state = scene.sim().team_script_vm.team(team).unwrap();
    assert_eq!(state.cursor(), 1);
    assert!(!state.advance_pending(), "Guard consumes the prior advance");
    let sim = &mut scene.runtime.simulation;
    let rules = &scene.runtime.resources.rules;
    // Supply a real uninterruptible Down through its owner. Script13 must
    // still complete while its unforced Do_Action request keeps this state.
    assert!(
        !crate::rules::infantry_sequence::action_record(DO_DOWN)
            .unwrap()
            .interruptible
    );
    assert!(
        sim.infantry_do_action(member, DO_DOWN, true, rules, FrameEffects::default())
            .unwrap()
    );
    let actor = sim.entities().get(member).unwrap();
    let prior = (actor.mission_leaf, *actor.native_stage(), actor.infantry);
    let rng = sim.rng_state();
    sim.team_run_action(
        team,
        TeamScriptAction {
            action_id: 13,
            argument: 1,
        },
        false,
        rules,
        None,
        FrameEffects::default(),
    );
    let actor = sim.entities().get(member).unwrap();
    assert_eq!(
        (actor.mission_leaf, *actor.native_stage(), actor.infantry),
        prior
    );
    assert_eq!(sim.rng_state(), rng);
    assert!(sim.team_script_vm.team(team).unwrap().advance_pending());
    let owner = sim.interner.resolve(actor.owner()).to_owned();
    for (kind, x) in [("HTNK", 40), ("ORCA", 45)] {
        let id = sim.spawn_object(kind, &owner, x, 85, 0, rules).unwrap();
        let actor = sim.entities().get(id).unwrap();
        let prior = (actor.mission_leaf, *actor.native_stage());
        let rng = sim.rng_state();
        sim.foot_play_idle_animation(id, 1, rules, FrameEffects::default());
        let actor = sim.entities().get(id).unwrap();
        assert_eq!((actor.mission_leaf, *actor.native_stage()), prior);
        assert_eq!(sim.rng_state(), rng);
    }
}

#[test]
fn campaign_triggered_team_plays_both_idle_variants_then_releases_its_member() {
    let Some((mut runtime, member, ty)) =
        super::scout_tests::campaign_scene("0=5,20\n1=13,-1\n2=5,4\n3=13,1\n4=5,4")
    else {
        return;
    };
    // Actual campaign startup/rules/Trigger4/recruitment on the shared authored
    // arena. This is a component history, not a retail mission playthrough.
    assert!(
        runtime
            .resources
            .rules
            .animation_sequence("E2")
            .unwrap()
            .infantry_action(DO_IDLE1)
            .unwrap()
            .frames_per_facing
            > 0
    );
    let mut observed = None;
    let mut first = false;
    let mut second = false;
    for _ in 0..600 {
        runtime.advance_frame_for_tooling(&[], 22).unwrap();
        let sim = &runtime.simulation;
        if let Some(team) = sim.team_script_vm.first_team_of_type(ty) {
            observed = Some(team.id());
            assert!(team.refusal().is_none(), "{:?}", team.refusal());
            if team.cursor() == 1 {
                assert_eq!(doing(sim, member), DO_IDLE1, "only argument1 selects Idle2");
                assert!(team.advance_pending());
                first = true;
            } else if team.cursor() == 3 {
                assert_eq!(doing(sim, member), DO_IDLE2);
                assert!(team.advance_pending());
                second = true;
            }
        } else if observed.is_some() {
            break;
        }
    }
    let sim = &runtime.simulation;
    assert!(first && second, "both parsed Script13 commands execute");
    assert!(sim.team_script_vm.team(observed.unwrap()).is_none());
    assert!(sim.team_script_vm.team_for_member(member).is_none());
    assert!(sim.entities().get(member).unwrap().lifecycle.object_alive());
}
