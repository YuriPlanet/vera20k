//! Script48 scouts the owner's unvisited House entries.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::house_state::HouseState;
use crate::sim::rng::SimRng;
use crate::sim::snapshot::GameSnapshot;

#[test]
fn scout_marks_a_house_without_buildings_then_completes() {
    let rules = RuleSet::from_ini(&IniFile::empty()).unwrap();
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Owner");
    let peer = sim.interner.intern("Peer");
    for house in [owner, peer] {
        sim.houses
            .register(HouseState::new(house, 0, None, false, 0, 10));
    }
    let script = sim.interner.intern("Scout");
    let team = sim.team_script_vm.create_team(owner, script, Vec::new(), 0);
    let action = TeamScriptAction {
        action_id: 48,
        argument: 0,
    };
    let unmarked_hash = sim.state_hash();
    sim.team_run_action(team, action, true, &rules, None, FrameEffects::default());
    assert!(!sim.team_script_vm.team(team).unwrap().advance_pending());
    assert!(sim.houses[&owner].has_scouted_house(peer));
    assert!(!sim.houses[&peer].has_scouted_house(owner));
    assert_ne!(sim.state_hash(), unmarked_hash);
    sim.scenario_rng = SimRng::new(0);
    let hash = sim.state_hash();
    let bytes = GameSnapshot::save(&sim, 0, 0, "scout", 0);
    sim = GameSnapshot::load(&bytes).unwrap().sim;
    assert_eq!(sim.state_hash(), hash);
    sim.team_run_action(team, action, false, &rules, None, FrameEffects::default());
    let state = sim.team_script_vm.team(team).unwrap();
    assert!(state.refusal().is_none());
    assert!(state.advance_pending(), "all peer houses have been scouted");
    let late = sim.interner.intern("Late");
    sim.houses
        .register(HouseState::new(late, 0, None, true, 0, 10));
    sim.team_run_action(team, action, false, &rules, None, FrameEffects::default());
    assert!(sim.houses[&owner].has_scouted_house(late));
    assert!(!sim.team_script_vm.team(team).unwrap().advance_pending());
}

#[test]
fn scout_draws_from_the_global_building_array_including_dead_and_limbo_entries() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art() else {
        return;
    };
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0);
    let owner = sim.interner.get("Russians").unwrap();
    let peer = sim.interner.get("Americans").unwrap();
    let third = sim.interner.intern("Third");
    sim.houses
        .register(HouseState::new(third, 0, None, true, 0, 10));
    let member = sim
        .spawn_object("ADOG", "Russians", 4, 4, 0, &rules)
        .unwrap();
    let limbo = sim
        .spawn_object("GAPOWR", "Americans", 12, 12, 0, &rules)
        .unwrap();
    let dead = sim
        .spawn_object("GAPOWR", "Americans", 20, 20, 0, &rules)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(limbo)
        .unwrap()
        .lifecycle
        .set_in_limbo_for_test(true);
    sim.substrate.entities.get_mut(dead).unwrap().health = crate::sim::combat::Health::new(0);
    let script = sim.interner.intern("Scout");
    let team = sim
        .team_script_vm
        .create_team(owner, script, vec![member], 0);
    sim.scenario_rng = SimRng::answering(0, 1, 0);
    let mut expected = sim.scenario_rng.clone();
    expected.next_range_i32_inclusive(0, 1); // two unmarked Houses
    expected.next_range_i32_inclusive(0, 1); // both registered Buildings
    sim.team_run_action(
        team,
        TeamScriptAction {
            action_id: 48,
            argument: 99,
        },
        false,
        &rules,
        None,
        FrameEffects::default(),
    );
    let state = sim.team_script_vm.team(team).unwrap();
    assert_eq!(state.scout_house, Some(peer));
    assert!(state.mission_target.is_some());
    assert!(!state.advance_pending());
    assert!(!sim.houses[&owner].has_scouted_house(peer));
    assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
    let selected_hash = sim.state_hash();
    sim.team_script_vm.teams.get_mut(&team).unwrap().scout_house = Some(third);
    assert_ne!(sim.state_hash(), selected_hash);
}

#[test]
fn retail_coop_scout_action_moves_after_reload_and_releases_its_team_slot() {
    use crate::rules::team_ai_ini::TeamAiIniRegistry;
    use crate::sim::team_script_vm::packed_cell_tests::{frame, reload, scene};
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let map =
        IniFile::from_bytes(assets.get_ref("C2S01MD.MAP").expect("retail co-op map")).unwrap();
    let registry = TeamAiIniRegistry::load_retail(&assets, &map, true).unwrap();
    let script = registry
        .scripts
        .iter()
        .find(|script| script.id == "09C1084C")
        .unwrap();
    let action = script.actions[0];
    assert_eq!((action.action_id, action.argument), (48, 0));
    let input = script
        .actions
        .iter()
        .enumerate()
        .map(|(index, action)| format!("{index}={},{}", action.action_id, action.argument))
        .collect::<Vec<_>>()
        .join("\n");
    let Some((mut scene, team, member)) = scene(&input, "ADOG", false, |scene| {
        let rules = &scene.runtime.resources.rules;
        let sim = &mut scene.runtime.simulation;
        let human = sim
            .houses
            .values()
            .find(|house| house.is_human())
            .unwrap()
            .name();
        let name = sim.interner.resolve(human).to_owned();
        sim.spawn_object("GAPOWR", &name, 38, 85, 0, rules).unwrap();
    }) else {
        return;
    };
    for _ in 0..120 {
        if scene
            .sim()
            .team_script_vm
            .team(team)
            .unwrap()
            .scout_house
            .is_some()
        {
            break;
        }
        frame(&mut scene);
    }
    let state = scene.sim().team_script_vm.team(team).unwrap();
    assert!(state.refusal().is_none());
    let peer = state
        .scout_house
        .expect("scout chose the House with a building");
    let target = state
        .mission_target
        .expect("scout assigned a passable destination");
    let owner = state.owner();
    let origin = scene.sim().entities().get(member).unwrap().position.clone();
    reload(&mut scene);
    let state = scene.sim().team_script_vm.team(team).unwrap();
    assert_eq!(state.scout_house, Some(peer));
    assert_eq!(state.mission_target, Some(target));
    for _ in 0..800 {
        frame(&mut scene);
        if scene.sim().team_script_vm.team(team).is_none() {
            break;
        }
    }
    let sim = scene.sim();
    assert!(sim.houses[&owner].has_scouted_house(peer));
    assert!(
        sim.team_script_vm.team(team).is_none(),
        "Scout reaches script EOF"
    );
    assert!(sim.team_script_vm.team_for_member(member).is_none());
    let actor = sim.entities().get(member).unwrap();
    assert!(actor.lifecycle.object_alive());
    assert_ne!(
        (actor.position.rx(), actor.position.ry()),
        (origin.rx(), origin.ry())
    );
}
