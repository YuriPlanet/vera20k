//! Script22 -> Map580BC0 with the campaign's selected visibility plane.

use super::*;
use crate::sim::runtime::SimRuntime;
use crate::sim::team_script_vm::packed_cell_tests::{frame, reload, scene};
use crate::sim::world::UninitContext;

#[test]
fn recruited_scout_keeps_its_selected_shrouded_cell_after_reload_then_finishes() {
    let Some((mut scene, team, member)) = scene("0=22,777", "E2", true, |scene| {
        let rules = &scene.runtime.resources.rules;
        // A real Action17 prior exposes the starting area on the player's
        // map, so scouting must select farther than ordinary Team Stray.
        assert!(
            scene
                .runtime
                .simulation
                .trigger_reveal_at_waypoint(697, UninitContext::with_rules(rules))
        );
    }) else {
        return;
    };
    let sim = scene.sim();
    let state = sim.team_script_vm.team(team).unwrap();
    assert!(state.refusal().is_none(), "{:?}", state.refusal());
    let target = state.mission_target.expect("Script22 selects a Cell");
    assert!(matches!(target, TeamTarget::Cell { .. }));
    let viewer = sim.session.current_house().unwrap();
    assert_ne!(
        viewer,
        state.owner(),
        "native query uses PlayerPtr, not Team House"
    );
    let point = sim.team_target_coord(target).unwrap();
    let cells = NativeCellQuery::canonical(sim.resolved_terrain().unwrap());
    assert!(crate::sim::vision::point_is_shrouded(
        &sim.fog, &cells, viewer, point
    ));
    assert!(
        !state.advance_pending(),
        "the selected unknown Cell requires movement"
    );
    reload(&mut scene);
    let rules = &scene.runtime.resources.rules;
    let sim = &mut scene.runtime.simulation;
    let before = sim.scenario_rng.logical_state();
    sim.team_step_script(team, rules, None, FrameEffects::default());
    assert_eq!(
        sim.team_script_vm.team(team).unwrap().mission_target,
        Some(target)
    );
    assert_eq!(
        sim.scenario_rng.logical_state(),
        before,
        "later visits do not reroll Map580BC0"
    );
    for _ in 0..1_000 {
        frame(&mut scene);
        if scene.sim().team_script_vm.team(team).is_none() {
            break;
        }
    }
    let sim = scene.sim();
    assert!(
        sim.team_script_vm.team(team).is_none(),
        "the scout arrives and reaches script EOF"
    );
    assert!(sim.team_script_vm.team_for_member(member).is_none());
    assert!(
        sim.object_distance_to(member, target.target_kind())
            .unwrap()
            <= scene.runtime.resources.rules.general.stray
    );
    let actor = sim.entities().get(member).unwrap();
    assert!(actor.lifecycle.object_alive());
    assert!(actor.navigation.nav_com().is_none());
}

#[test]
fn a_revealed_player_map_returns_the_scout_to_its_center_without_a_search_draw() {
    let Some((mut scene, team, member)) = scene("0=22,-1", "E2", false, |_| {}) else {
        return;
    };
    let sim = &mut scene.runtime.simulation;
    let rules = &scene.runtime.resources.rules;
    let state = sim.team_script_vm.team(team).unwrap();
    assert!(state.refusal().is_none(), "{:?}", state.refusal());
    let center = state.zone.unwrap();
    assert_eq!(state.mission_target, Some(center));
    assert!(state.advance_pending());
    let before = sim.scenario_rng.logical_state();
    sim.team_run_action(
        team,
        TeamScriptAction {
            action_id: 22,
            argument: 912,
        },
        true,
        rules,
        None,
        FrameEffects::default(),
    );
    assert_eq!(
        sim.scenario_rng.logical_state(),
        before,
        "no shrouded candidate means no Scenario RNG request"
    );
    assert_eq!(
        sim.team_script_vm.team(team).unwrap().mission_target,
        Some(center)
    );
    frame(&mut scene);
    assert!(scene.sim().team_script_vm.team(team).is_none());
    assert!(
        scene
            .sim()
            .entities()
            .get(member)
            .unwrap()
            .lifecycle
            .object_alive()
    );
}

#[test]
fn authored_lan_scout_uses_first_primary_even_on_a_guest_view() {
    let Some((mut runtime, member, ty)) = campaign_scene("0=22,0", "E2") else {
        return;
    };
    let sim = &mut runtime.simulation;
    let rules = &runtime.resources.rules;
    let first = sim.houses.order()[0];
    let guest = sim.houses.order()[1];
    sim.session.retire_started();
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Lan);
    sim.session.install_authored_campaign(vec![1, 0], false);
    sim.session.install_current_house(Some(guest));
    for owner in [first, guest] {
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .set_is_human_for_test(true);
    }
    let terrain = sim.resolved_terrain().unwrap();
    sim.fog = crate::sim::vision::FogState::new(terrain.width(), terrain.height());
    sim.reveal_entire_map(Some(guest), UninitContext::with_rules(rules));

    let team = sim.create_team(ty, Some(first), rules).unwrap();
    assert!(sim.team_add_member(team, member, false, rules, None, FrameEffects::default()));
    let before = sim.scenario_rng.logical_state();
    sim.team_run_action(
        team,
        TeamScriptAction {
            action_id: 22,
            argument: 0,
        },
        true,
        rules,
        None,
        FrameEffects::default(),
    );

    let state = sim.team_script_vm.team(team).unwrap();
    assert!(state.refusal().is_none());
    assert_ne!(sim.scenario_rng.logical_state(), before);
    let target = state.mission_target.unwrap();
    let point = sim.team_target_coord(target).unwrap();
    let cells = NativeCellQuery::canonical(sim.resolved_terrain().unwrap());
    assert!(crate::sim::vision::point_is_shrouded(
        &sim.fog, &cells, first, point
    ));
    assert!(!crate::sim::vision::point_is_shrouded(
        &sim.fog, &cells, guest, point
    ));
}

pub(super) fn campaign_scene(
    script: &str,
    member_type: &str,
) -> Option<(SimRuntime, u64, InternedId)> {
    use crate::map::map_file::MapFile;
    use crate::rules::process_owner::NativeScenarioRulesPrefix;
    use crate::rules::team_ai_ini::TeamAiIniRegistry;
    use crate::sim::campaign_carry_over::CampaignCarryOver;
    use crate::sim::runtime::SimResources;
    use crate::sim::scenario_bootstrap as bootstrap;

    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets()?;
    // An authored campaign arena, not a retail mission playthrough. The
    // campaign rules passes, Basic.Player, Trigger4 and recruitment are real.
    let map = MapFile::from_bytes(format!("[Basic]\nNewINIFormat=4\nPlayer=Americans\n\
        [Map]\nTheater=TEMPERATE\nSize=0,0,32,32\nLocalSize=2,2,28,24\n\
        [IsoMapPack5]\n1=CAAEABUAAAAAEQAA\n\
        [Houses]\n0=Russians\n1=Americans\n[Russians]\nCountry=Russians\n[Americans]\nCountry=Americans\n\
        [Waypoints]\n1=20025\n\
        [Infantry]\nSCOUT=Russians,{member_type},256,20,20,0,Guard,0,None,0,-1,0,1,1\n\
        [TeamTypes]\n0=SCOUT-TEAM\n[SCOUT-TEAM]\nHouse=Russians\nScript=SCOUT-SCRIPT\nTaskForce=SCOUT-FORCE\nGroup=-1\nMax=1\n\
        [ScriptTypes]\n0=SCOUT-SCRIPT\n[SCOUT-SCRIPT]\n{script}\n\
        [TaskForces]\n0=SCOUT-FORCE\n[SCOUT-FORCE]\nGroup=-1\n0=1,{member_type}\n\
        [Triggers]\nSCOUT-TRIGGER=Russians,<none>,Scout,0,1,1,1,0\n\
        [Events]\nSCOUT-TRIGGER=1,13,0,1\n[Actions]\nSCOUT-TRIGGER=1,4,1,SCOUT-TEAM,0,0,0,0,A\n\
        [Tags]\nSCOUT-TAG=0,Scout,SCOUT-TRIGGER\n").as_bytes()).unwrap();
    let (mut rules, _, art, _) = crate::rules::retail_ini_fixture::retail_rules_owner(&assets)
        .load_scenario(NativeScenarioRulesPrefix::Campaign(None), &map.ini)
        .unwrap()
        .into_parts();
    // The production loader installs fixed ART and binds its action bank
    // before actors; the sequence binder resolves each type through that ART.
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    let teams = TeamAiIniRegistry::load_retail(&assets, &map.ini, false).unwrap();
    let roster =
        crate::map::houses::parse_house_roster(&map.ini, &rules.color_schemes, Some(&rules));
    let mut sim = Simulation::with_seed(0x580BC0);
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session
        .initialize_campaign_startup(1, 1, None, 0, CampaignCarryOver::CLEARED)
        .unwrap();
    bootstrap::initialize_map_roster_houses(&mut sim, &roster, &rules, Some(&map.ini));
    bootstrap::initialize_campaign_current_house(&mut sim.session, &mut sim.houses, &map.ini);
    sim.install_team_ai_registry(&teams, &rules);
    sim.initialize_trigger_definitions(&map, &teams, None, None, None);
    sim.session.install_waypoints(&map.waypoints);
    assert_eq!(sim.spawn_from_map(&map.entities, Some(&rules)), 1);
    // Full_Init's Fill registers unattached timed Tags after the map actors;
    // definitions alone do not put Event13 into Logic's update list.
    sim.classify_map_triggers(&rules);
    sim.initialize_scenario_sight(UninitContext::with_rules(&rules));
    sim.initialize_fog_of_war(UninitContext::with_rules(&rules));
    let member = sim.entities().values().next().unwrap().stable_id();
    let ty = sim.interner.get("SCOUT-TEAM").unwrap();
    sim.session.publish_started();
    let runtime = SimRuntime {
        simulation: sim,
        resources: std::sync::Arc::new(SimResources::new(rules)),
    };
    Some((runtime, member, ty))
}

#[test]
fn campaign_startup_runs_an_authored_scout_through_its_timed_trigger() {
    let Some((mut runtime, member, ty)) = campaign_scene("0=22,0", "E2") else {
        return;
    };
    let mut observed = None;
    let mut scouted = false;
    for _ in 0..500 {
        runtime.advance_frame_for_tooling(&[], 22).unwrap();
        let sim = &runtime.simulation;
        if let Some(team) = sim.team_script_vm.first_team_of_type(ty) {
            observed = Some(team.id());
            assert!(team.refusal().is_none(), "{:?}", team.refusal());
            if team.cursor() == 0 {
                assert_eq!(team.members().collect::<Vec<_>>(), [member]);
                assert!(matches!(team.mission_target, Some(TeamTarget::Cell { .. })));
                scouted = true;
            }
        } else if observed.is_some() {
            break;
        }
    }
    assert!(scouted, "the campaign Team executes its scouting action");
    let team = observed.expect("the campaign timed Trigger4 creates its Team");
    assert!(runtime.simulation.team_script_vm.team(team).is_none());
    assert!(
        runtime
            .simulation
            .team_script_vm
            .team_for_member(member)
            .is_none()
    );
    assert!(
        runtime
            .simulation
            .entities()
            .get(member)
            .unwrap()
            .lifecycle
            .object_alive()
    );
}
