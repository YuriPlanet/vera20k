//! Script4/6E95B3 through parsed Trigger4, recruitment and ordinary movement.

use super::*;
use crate::headless_scenario::{HeadlessScenario, battle_session, load_session};
use crate::rules::ini_parser::IniFile;
use crate::rules::team_ai_ini::TeamAiIniRegistry;
use crate::sim::combat::TargetKind;
use crate::sim::house_state::HouseState;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::{FrameEffects, Simulation, SnapshotProcessState, TickLane};
use crate::skirmish_launch::{AiDifficulty, LaunchCountry, LaunchStartPosition};

pub(super) fn frame(scene: &mut HeadlessScenario) {
    let commands = scene.runtime.simulation.take_due_commands();
    let result = scene
        .runtime
        .advance_frame(&commands, 22, TickLane::Ordinary, FrameEffects::default())
        .unwrap();
    assert!(!result.outcome_waiting);
}

pub(super) fn scene(
    script: &str,
    shroud: bool,
    prepare: impl FnOnce(&mut HeadlessScenario),
) -> Option<(HeadlessScenario, u64, u64)> {
    let (root, assets) = crate::rules::retail_ini_fixture::retail_assets()?;
    let stock = assets.resolve_ref("XMP03T4.MAP").unwrap();
    let stock = std::str::from_utf8(&stock.bytes).unwrap();
    // Retail already defines98/99. Duplicate-key CRC selection changes
    // when an entry is added, so author unused slots instead of shadowing.
    let stock_ini = IniFile::from_str(stock);
    for index in ["697", "700", "701"] {
        assert!(!stock_ini.section_or_empty("Waypoints").is_present(index));
    }
    let mut map = super::firesale_retail_tests::scenario(stock, script);
    super::firesale_retail_tests::insert_section_records(
        &mut map,
        "Waypoints",
        "697=85030\n700=-2\n701=-3\n",
    );
    let directory = crate::map::source::test_support::TestDirectory::new("team-packed-cell");
    directory.write("packed-cell.map", map.as_bytes());
    let path = directory.path().join("packed-cell.map");
    let name = path.to_str().unwrap();
    let mut launch = battle_session(name, &[(LaunchCountry::Russia, AiDifficulty::Easy)]);
    launch.local_slot_mut().start_position = LaunchStartPosition::Position(2);
    launch.opponents[0].start_position = LaunchStartPosition::Position(1);
    launch.options.starting_credits = 5000;
    launch.options.unit_count = 0;
    launch.options.short_game = false;
    launch.options.crates = false;
    launch.options.shroud = shroud;
    let mut scene = load_session(&root, name, 0x6E95B3, launch).unwrap();
    prepare(&mut scene);
    let sim = scene.sim();
    let owner = Simulation::scenario_house_named(&sim.houses, "Computer").unwrap();
    let member = sim
        .entities()
        .values()
        .find(|entity| {
            entity.owner() == owner
                && sim.interner.resolve(entity.type_ref()) == "E2"
                && (entity.position.rx(), entity.position.ry()) == (30, 85)
        })
        .unwrap()
        .stable_id();
    let team_type = sim.interner.get("F0520001").unwrap();
    for _ in 0..120 {
        frame(&mut scene);
        if let Some(team) = scene.sim().team_script_vm.first_team_of_type(team_type)
            && team.formed()
            && team.cursor() == 0
        {
            assert_eq!(team.members().collect::<Vec<_>>(), [member]);
            let team = team.id();
            return Some((scene, team, member));
        }
    }
    panic!("timed Trigger4 did not create and recruit its team");
}

pub(super) fn reload(scene: &mut HeadlessScenario) {
    let runtime = &mut scene.runtime;
    let sim = &runtime.simulation;
    let rules = &runtime.resources.rules;
    let map_hash = scene.map.ini.content_hash();
    let rules_hash = rules.simulation_config_hash();
    let bytes = GameSnapshot::save_validated(sim, map_hash, rules_hash, "packed-cell", 0);
    let mut loaded =
        GameSnapshot::load_validated(&bytes, map_hash, rules_hash, sim.session.map_name())
            .unwrap()
            .sim;
    loaded.retain_process_state_for_load(SnapshotProcessState::Live(sim));
    loaded.bind_shared_cell_dummy(crate::map::resolved_terrain::SharedCellDummy::fresh());
    loaded.reconstruct_cellclass_dummy_for_map_resize();
    loaded.restore_after_snapshot_load().unwrap();
    crate::sim::production::validate_restored_factory_state(&loaded, rules).unwrap();
    loaded.rebuild_caches_after_load(
        sim.resolved_terrain().unwrap().clone(),
        sim.terrain_speed_config().clone(),
        rules,
    );
    loaded
        .restore_map_authority_after_snapshot_load(rules, &runtime.resources.overlay_registry, None)
        .unwrap();
    loaded.intern_rule_type_ids(rules);
    loaded.resolve_type_handles(rules);
    loaded.rebuild_lighting_sources_after_load(rules);
    let dummy = sim.effective_shared_cell_dummy();
    dummy.adopt_prepared_load_state(&loaded.effective_shared_cell_dummy());
    loaded.bind_shared_cell_dummy(dummy);
    loaded.install_fresh_process_main(sim.main_rng());
    loaded.publish_scenario_started();
    runtime.replace_simulation(loaded);
}

#[test]
fn authored_packed_cell_team_moves_after_reload_and_finishes_at_eof() {
    // Signed packed128 coordinates, not a waypoint number (6E95B7..6E95E5).
    let target = TeamTarget::Cell { x: 36, y: 85 };
    let Some((mut scene, team, member)) = scene(&format!("0=4,{}", 85 * 128 + 36), false, |_| {})
    else {
        return;
    };
    let state = scene.sim().team_script_vm.team(team).unwrap();
    assert!(state.refusal().is_none(), "{:?}", state.refusal());
    assert_eq!(state.mission_target, Some(target));
    assert!(!state.advance_pending());
    assert!(
        scene
            .sim()
            .entities()
            .get(member)
            .unwrap()
            .navigation
            .nav_com()
            .is_some_and(
                |nav| nav.same_pointer(target.target_kind().into(), scene.sim().resolved_terrain())
            )
    );
    let origin = scene.sim().entities().get(member).unwrap().position.clone();
    reload(&mut scene);
    assert_eq!(
        scene
            .sim()
            .team_script_vm
            .team(team)
            .unwrap()
            .mission_target,
        Some(target)
    );
    for _ in 0..600 {
        frame(&mut scene);
        if scene.sim().team_script_vm.team(team).is_none() {
            break;
        }
    }
    let sim = scene.sim();
    assert!(
        sim.team_script_vm.team(team).is_none(),
        "CoordinateMove must complete Script4"
    );
    assert!(sim.team_script_vm.team_for_member(member).is_none());
    assert!(
        sim.object_distance_to(member, target.target_kind())
            .unwrap()
            <= scene.runtime.resources.rules.general.stray
    );
    let member = sim.entities().get(member).unwrap();
    assert!(member.lifecycle.object_alive());
    assert_ne!(
        (member.position.rx(), member.position.ry()),
        (origin.rx(), origin.ry())
    );
    assert!(
        member.navigation.nav_com().is_none(),
        "completed Move releases its destination"
    );
}

#[test]
fn packed_cell_dummy_keeps_its_pointer_across_restamps_and_reload() {
    let Some((mut scene, team, member)) = scene("0=4,-1", false, |_| {}) else {
        return;
    };
    let target = TeamTarget::Cell { x: -1, y: 0 };
    assert_eq!(
        scene
            .sim()
            .team_script_vm
            .team(team)
            .unwrap()
            .mission_target,
        Some(target)
    );
    let expected = scene.sim().trigger_waypoint_location(700).unwrap();
    let current = scene.sim().team_target_coord(target).unwrap();
    assert_eq!(
        [current.x, current.y],
        [expected[0], expected[1]],
        "retained Cell+48 reads the current Dummy without restamping"
    );
    reload(&mut scene);
    let sim = &mut scene.runtime.simulation;
    let rules = &scene.runtime.resources.rules;
    let expected = sim.trigger_waypoint_location(701).unwrap();
    let current = sim.team_target_coord(target).unwrap();
    assert_eq!([current.x, current.y], [expected[0], expected[1]]);
    assert_eq!(
        sim.team_script_vm.team(team).unwrap().mission_target,
        Some(target)
    );
    sim.team_member_assign_target(member, target, rules, None, FrameEffects::default());
    let changed_token = sim.canonical_team_cell((-2, 0));
    sim.team_assign_mission_target(
        team,
        Some(changed_token),
        rules,
        None,
        FrameEffects::default(),
    );
    assert_eq!(
        sim.entities().get(member).unwrap().attack_target.target(),
        Some(TargetKind::Cell(u16::MAX, 0)),
        "same Dummy pointer must not clear the member's target (6E905C)"
    );
}

#[test]
fn a_team_only_dummy_target_makes_its_live_coordinate_part_of_the_hash() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    let owner = sim.interner.intern("Owner");
    sim.houses
        .register(HouseState::new(owner, 0, None, false, 0, 10));
    let map = IniFile::from_str(
        "[TeamTypes]\n0=TEAM\n[TEAM]\nScript=SCRIPT\nTaskForce=FORCE\n[ScriptTypes]\n0=SCRIPT\n[SCRIPT]\n0=2,0\n[TaskForces]\n0=FORCE\n[FORCE]\n0=1,E2\n",
    );
    let registry = TeamAiIniRegistry::from_sources(&IniFile::empty(), &map, false);
    sim.install_team_ai_registry(&registry, &rules).unwrap();
    let team_type = sim.interner.get("TEAM").unwrap();
    let team = sim.create_team(team_type, Some(owner), &rules).unwrap();
    let target = sim.canonical_team_cell((-1, 0));
    sim.team_assign_mission_target(team, Some(target), &rules, None, FrameEffects::default());
    let before = sim.state_hash();
    let _ = sim.canonical_team_cell((-2, 0));
    assert_ne!(
        sim.state_hash(),
        before,
        "Team-held Dummy changes future GetCoords and movement even with no entity retaining it"
    );
}
