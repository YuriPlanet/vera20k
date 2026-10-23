use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::scenario_session::ScenarioGameMode;
use crate::sim::snapshot::GameSnapshot;

#[test]
fn pending_group_assignment_survives_save_and_commits_as_local_ingress() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n[E1]\nStrength=100\n",
    ))
    .unwrap();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 731060);
    sim.session.set_game_mode(ScenarioGameMode::Skirmish);
    let owner = sim.interner.get("Americans").unwrap();
    sim.session.set_current_house_for_test(Some(owner));
    let first = sim
        .spawn_object("E1", "Americans", 10, 10, 0, &rules)
        .unwrap();
    let second = sim
        .spawn_object("E1", "Americans", 11, 10, 0, &rules)
        .unwrap();
    let enemy = sim
        .spawn_object("E1", "Russians", 20, 20, 0, &rules)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(first)
        .unwrap()
        .assign_object_group(0);
    sim.substrate
        .entities
        .get_mut(enemy)
        .unwrap()
        .assign_object_group(0);
    let command = CommandEnvelope::new(
        owner,
        0,
        Command::AssignControlGroup {
            group: 0,
            entity_ids: vec![second, enemy],
        },
    );
    assert!(sim.issue_local_command(command.clone()));
    assert!(
        sim.local_commands.is_empty(),
        "native assigns no EventClass slot"
    );
    assert_eq!(sim.entities().get(second).unwrap().object_group(), -1);
    assert_eq!(sim.projected_control_groups()[1], [second, enemy]);

    let bytes = GameSnapshot::save(&sim, 0, 0, "group-ingress", 0);
    let mut loaded = GameSnapshot::load(&bytes).unwrap().sim;
    loaded.restore_after_snapshot_load().unwrap();
    assert_eq!(loaded.entities().get(first).unwrap().object_group(), 0);
    assert_eq!(loaded.projected_control_groups()[1], [second, enemy]);
    let pending = loaded.take_due_commands();
    assert_eq!(pending, [command]);
    let rng_before = loaded.scenario_rng.clone();
    let checksum_before = loaded.compute_retail_multiplayer_checksum().unwrap().value;
    loaded.scenario_rng = rng_before;
    let hash_before = loaded.state_hash();
    assert_eq!(
        loaded.apply_due_frame_ingress_commands(&pending, &rules, 1, FrameEffects::default()),
        1
    );
    assert_eq!(loaded.entities().get(first).unwrap().object_group(), -1);
    assert_eq!(loaded.entities().get(second).unwrap().object_group(), 0);
    assert_eq!(
        loaded.entities().get(enemy).unwrap().object_group(),
        0,
        "ineligible owner retains its field"
    );
    assert_ne!(
        loaded.state_hash(),
        hash_before,
        "Group participates in replay state"
    );
    assert_eq!(
        loaded.compute_retail_multiplayer_checksum().unwrap().value,
        checksum_before,
        "native frame checksum excludes local Group"
    );
}
