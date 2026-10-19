//! Team19/41 call Infantry521C10/521C40; the latter clears fear only.
//! The formed-Team seam does not stand for recruitment or gathering.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::team_ai_ini::{TeamAiDefinitionSource, TeamAiIniRegistry};
use crate::sim::team_script_vm::{TeamScriptMember, member_type_identity};

const SCRIPT: &str = "\
[ScriptTypes]
0=E1PanicReset
[E1PanicReset]
0=19,0
1=41,0
2=24,0
[TaskForces]
0=E1PanicForce
[E1PanicForce]
Group=-1
0=1,E1
[TeamTypes]
0=E1PanicTeam
[E1PanicTeam]
House=Americans
Script=E1PanicReset
TaskForce=E1PanicForce
Max=1
Recruiter=no
Autocreate=no
";

fn scene(extra: &str) -> Option<(RuleSet, Simulation, u64, u64)> {
    let patch = IniFile::from_str(&format!("{SCRIPT}\n{extra}"));
    let retail = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
        "XMP03T4.MAP",
        &patch,
    )?;
    let (root, assets) = crate::rules::retail_ini_fixture::retail_scenario_assets()?;
    let mut map =
        crate::map::source::load_map_by_name_or_path_with_assets(&root, "XMP03T4.MAP", &assets)
            .unwrap()
            .map;
    map.ini.merge(&patch);
    let registry = TeamAiIniRegistry::load_retail(&assets, &map.ini, true).unwrap();
    assert_eq!(
        registry
            .scripts
            .iter()
            .filter(|script| script.source == TeamAiDefinitionSource::FixedAimd)
            .flat_map(|script| &script.actions)
            .filter(|action| matches!(action.action_id, 19 | 41))
            .count(),
        0,
        "this is an authored map route; fixed retail AIMD does not supply it"
    );
    let rules = retail.rules;
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x521C40);
    sim.session.game_options.short_game = false;
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.intern_rule_type_ids(&rules);
    sim.install_team_ai_registry(&registry, &rules).unwrap();
    let owner = sim.interner.get("Americans").unwrap();
    sim.session.current_house = Some(owner);
    let actor = sim
        .spawn_object("E1", "Americans", 12, 12, 0, &rules)
        .unwrap();
    sim.resolve_type_handles(&rules);
    let member = TeamScriptMember {
        entity_id: actor,
        member_type: member_type_identity(sim.entities().get(actor).unwrap()),
    };
    let kind = sim.interner.get("E1PanicTeam").unwrap();
    let team = sim.team_script_vm.create_team_from_type(
        owner,
        kind,
        &[member],
        sim.session.binary_frame as i32,
    );
    assert!(sim.team_script_vm.team(team).unwrap().formed());
    Some((rules, sim, team, actor))
}

#[test]
fn unpanic_uses_the_type_gate_and_preserves_stance_and_clocks() {
    for (patch, rank_percent, panic_fear, unpanic_fear) in [
        ("", 0, 300, 0),
        ("[E1]\nFearless=yes\n", 0, 77, 77),
        ("[E1]\nFearless=no\nVeteranAbilities=FEARLESS\n", 100, 77, 0),
        ("[E1]\nFearless=no\nEliteAbilities=FEARLESS\n", 200, 77, 0),
    ] {
        let Some((rules, mut sim, team, actor)) = scene(patch) else {
            return;
        };
        // Callback controls supply existing fear/prone/clock state. In
        // particular, Panic cannot produce positive fear on a Fearless type.
        let entity = sim.substrate.entities.get_mut(actor).unwrap();
        entity.veterancy.set_rank_for_test(rank_percent);
        let infantry = entity.infantry.as_mut().unwrap();
        infantry.set_fear_level_for_test(77);
        infantry.set_is_prone_for_test(true);
        let mut expected = *infantry;
        entity.restart_native_stage(7, 17, 92);
        let stage = *entity.native_stage();
        let leaf = entity.mission_leaf;
        let rng = sim.rng_state();

        sim.team_step_script(team, &rules, None, FrameEffects::default());
        expected.set_fear_level_for_test(panic_fear);
        assert_eq!(sim.entities().get(actor).unwrap().infantry, Some(expected));
        sim.team_step_script(team, &rules, None, FrameEffects::default());
        expected.set_fear_level_for_test(unpanic_fear);
        let entity = sim.entities().get(actor).unwrap();
        assert_eq!(entity.infantry, Some(expected), "{patch}");
        assert_eq!(
            *entity.native_stage(),
            stage,
            "UnPanic does not restart Stage"
        );
        assert_eq!(entity.mission_leaf, leaf);
        assert_eq!(sim.rng_state(), rng, "the callbacks draw nothing");
        let state = sim.team_script_vm.team(team).unwrap();
        assert_eq!(state.cursor(), 1);
        assert!(state.advance_pending());
        assert!(state.refusal().is_none());
    }
    assert!(action_is_ported(41));
}

fn tick(sim: &mut Simulation, rules: &RuleSet) {
    let commands = sim.take_due_commands();
    let grid = sim.path_grid_snapshot();
    sim.advance_tick(&commands, Some(rules), grid.as_deref(), None, 33);
}

#[test]
fn retail_map_script_unpanics_then_releases_the_surviving_gi() {
    let Some((rules, mut sim, team, actor)) = scene("") else {
        return;
    };
    assert!(!rules.object("E1").unwrap().fearless);
    let health = sim.entities().get(actor).unwrap().health.current();

    tick(&mut sim, &rules);
    let infantry = sim.entities().get(actor).unwrap().infantry.unwrap();
    assert_eq!(
        infantry.fear_level(),
        299,
        "Team Panic precedes ordinary fear AI"
    );
    assert!(
        infantry.is_prone(),
        "the existing Down owner accepts stock GI fear"
    );
    assert_eq!(sim.team_script_vm.team(team).unwrap().cursor(), 0);

    tick(&mut sim, &rules);
    let infantry = sim.entities().get(actor).unwrap().infantry.unwrap();
    assert_eq!(
        infantry.fear_level(),
        0,
        "the next Team visit calls UnPanic"
    );
    assert!(
        infantry.is_prone(),
        "direct zero is not natural positive-fear decay"
    );
    assert_eq!(sim.team_script_vm.team(team).unwrap().cursor(), 1);

    tick(&mut sim, &rules);
    assert_eq!(sim.team_script_vm.team(team).unwrap().cursor(), 2);
    assert!(sim.team_script_vm.team(team).unwrap().advance_pending());
    tick(&mut sim, &rules);
    assert!(
        sim.team_script_vm.team(team).is_none(),
        "EOF destroys the Team"
    );
    assert!(sim.team_script_vm.team_for_member(actor).is_none());
    for _ in 0..8 {
        tick(&mut sim, &rules);
        let entity = sim
            .entities()
            .get(actor)
            .expect("Team release does not UnInit the GI");
        assert!(entity.lifecycle.object_alive());
        assert!(!entity.lifecycle.in_limbo());
        assert_eq!(entity.health.current(), health);
        assert_eq!(entity.infantry.unwrap().fear_level(), 0);
    }
    sim.occupancy()
        .validate_memberships(sim.entities())
        .unwrap();
}
