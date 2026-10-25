//! Script15/6ECE60 -> CoordinateAttack6EB490 -> ordinary spy entry/blackout.

use super::*;
use crate::sim::runtime::SimRuntime;
use crate::sim::spy_infiltration::InfiltrationEffect;
use crate::sim::team_script_vm::packed_cell_tests::{frame, reload, scene};
use crate::sim::world::{SimSoundEvent, TickLane, UninitContext};

fn enter_power_plant(runtime: &mut SimRuntime, team: u64, spy: u64, cell: (u16, u16)) {
    let sim = &mut runtime.simulation;
    let rules = &runtime.resources.rules;
    let owner = sim.session.current_house().unwrap();
    let attacker = sim.entities().get(spy).unwrap().owner();
    let owner_name = sim.interner.resolve(owner).to_owned();
    let kind = rules.object("GAPOWR").unwrap();
    assert!(kind.spyable && kind.power > 0 && !kind.radar);
    assert!(rules.object("SPY").unwrap().infiltrate);
    // A late scenario actor, through construction/Unlimbo. No Capture command,
    // target, navigation, membership, health or blackout state is supplied.
    let plant = sim
        .spawn_object("GAPOWR", &owner_name, cell.0, cell.1, 0, rules)
        .unwrap();
    let health = sim.entities().get(plant).unwrap().health;
    let mut captured = false;
    let mut effects = 0;
    for _ in 0..600 {
        let commands = runtime.simulation.take_due_commands();
        let output = runtime
            .advance_frame(&commands, 22, TickLane::Ordinary, FrameEffects::default())
            .unwrap();
        effects += output.sound_events.iter().filter(|event| matches!(event,
            SimSoundEvent::BuildingInfiltrated { owner: target, infiltrator, effect: InfiltrationEffect::Power, .. }
                if *target == owner && *infiltrator == attacker
        )).count();
        let sim = &runtime.simulation;
        if let Some(actor) = sim.entities().get(spy) {
            captured |= actor.mission.current().known() == Some(MissionType::Capture);
        } else {
            break;
        }
    }
    let sim = &runtime.simulation;
    assert!(captured, "the team's order must actually admit Capture");
    assert_eq!(effects, 1, "one real building entry applies the Power arm");
    assert!(
        sim.entities().get(spy).is_none(),
        "Agent entry consumes the spy"
    );
    assert!(sim.team_script_vm.team_for_member(spy).is_none());
    // RemoveMember6EA870 marks the emptied Team altered. The next TeamAI
    // recount6EA3E0 destroys it; consumption happens after this frame's pass.
    let emptied = sim.team_script_vm.team(team).unwrap();
    assert!(emptied.members().next().is_none() && emptied.altered);
    assert_eq!(sim.entities().get(plant).unwrap().owner(), owner);
    assert_eq!(sim.entities().get(plant).unwrap().health, health);
    assert!(sim.power_states[&owner].blackout_remaining(sim.session.binary_frame()) > 0);
    // Ordinary House visits apply and expire the retail duration, with no
    // timer seeding. The same power owner serves AI and player commands.
    let mut dark = false;
    for _ in 0..=runtime.resources.rules.general.spy_power_blackout {
        runtime.advance_frame_for_tooling(&[], 22).unwrap();
        let sim = &runtime.simulation;
        assert!(
            sim.team_script_vm.team(team).is_none(),
            "next Team visit cleans up"
        );
        let power = &sim.power_states[&owner];
        if power.blackout_remaining(sim.session.binary_frame()) > 0 {
            assert_eq!(power.total_output(), 0);
            dark = true;
        } else {
            assert!(
                power.total_output() > 0,
                "House recovers the live power plant"
            );
            assert!(dark);
            return;
        }
    }
    panic!("House never expired its spy blackout");
}

#[test]
fn recruited_spy_waits_at_empty_waypoint_across_reload_then_infiltrates() {
    let Some((mut scene, team, spy)) = scene("0=15,698", "SPY", false, |_| {}) else {
        return;
    };
    let target = Some(TeamTarget::Cell { x: 36, y: 85 });
    for _ in 0..20 {
        let state = scene.sim().team_script_vm.team(team).unwrap();
        assert!(state.refusal().is_none(), "{:?}", state.refusal());
        assert_eq!(state.mission_target, target);
        assert!(
            !state.advance_pending(),
            "an empty waypoint waits for a blocker"
        );
        frame(&mut scene);
    }
    reload(&mut scene);
    assert_eq!(
        scene
            .sim()
            .team_script_vm
            .team(team)
            .unwrap()
            .mission_target,
        target
    );
    enter_power_plant(&mut scene.runtime, team, spy, (36, 85));
}

#[test]
fn campaign_trigger_recruits_spy_and_its_waypoint_order_reaches_building_entry() {
    let Some((mut runtime, spy, ty)) = super::scout_tests::campaign_scene("0=15,1", "SPY") else {
        return;
    };
    // Real campaign rules/startup/Player/Trigger4 on the existing authored
    // arena; this component history is not a retail mission playthrough.
    let mut formed = None;
    for _ in 0..120 {
        runtime.advance_frame_for_tooling(&[], 22).unwrap();
        if let Some(team) = runtime.simulation.team_script_vm.first_team_of_type(ty)
            && team.formed()
            && team.cursor() == 0
        {
            assert!(team.refusal().is_none(), "{:?}", team.refusal());
            assert_eq!(team.members().collect::<Vec<_>>(), [spy]);
            assert_eq!(team.mission_target, Some(TeamTarget::Cell { x: 25, y: 20 }));
            assert!(!team.advance_pending());
            formed = Some(team.id());
            break;
        }
    }
    enter_power_plant(
        &mut runtime,
        formed.expect("timed Trigger4 forms the spy Team"),
        spy,
        (25, 20),
    );
}

#[test]
fn expired_spy_target_advances_without_discovering_its_replacement() {
    let Some((mut scene, team, spy)) = scene("0=15,698", "SPY", false, |_| {}) else {
        return;
    };
    assert!(
        scene
            .sim()
            .team_script_vm
            .team(team)
            .unwrap()
            .refusal()
            .is_none()
    );
    let owner = scene.sim().session.current_house().unwrap();
    let owner_name = scene.sim().interner.resolve(owner).to_owned();
    let rules = &scene.runtime.resources.rules;
    let plant = scene
        .runtime
        .simulation
        .spawn_object("GAPOWR", &owner_name, 36, 85, 0, rules)
        .unwrap();
    frame(&mut scene);
    reload(&mut scene);
    assert_eq!(
        scene
            .sim()
            .team_script_vm
            .team(team)
            .unwrap()
            .mission_target,
        Some(TeamTarget::Object(plant))
    );
    // Focused lifecycle control through the production UnInit broadcaster.
    // A subsequent Script15 visit sees NULL, not the original waypoint Cell.
    let rules = &scene.runtime.resources.rules;
    let sim = &mut scene.runtime.simulation;
    sim.uninit_with_context(plant, UninitContext::with_rules(rules));
    assert!(
        sim.team_script_vm
            .team(team)
            .unwrap()
            .mission_target
            .is_none()
    );
    let replacement = sim
        .spawn_object("GAPOWR", &owner_name, 36, 85, 0, rules)
        .unwrap();
    for _ in 0..30 {
        frame(&mut scene);
        if scene.sim().team_script_vm.team(team).is_none() {
            break;
        }
    }
    let sim = scene.sim();
    assert!(
        sim.team_script_vm.team(team).is_none(),
        "NULL advances to EOF"
    );
    assert!(sim.team_script_vm.team_for_member(spy).is_none());
    assert!(sim.entities().get(spy).unwrap().lifecycle.object_alive());
    assert_eq!(sim.entities().get(replacement).unwrap().owner(), owner);
    assert_eq!(
        sim.power_states[&owner].blackout_remaining(sim.session.binary_frame()),
        0
    );
}
