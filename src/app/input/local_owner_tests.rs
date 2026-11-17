//! App identity after the real prepare/commit load transaction, without GPU state.
use super::schedule_command_in_sim;
use crate::app::match_runtime::state::scenario_local_owner;
use crate::app::persistence::{LoadPreparationView, PreparedLoad, SaveRepository};
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::command::Command;
use crate::sim::house_state::HouseState;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::runtime::SimRuntime;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::Simulation;

#[test]
fn saved_current_house_drives_app_fog_and_command_owner_after_load() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n[Warheads]\n[OverlayTypes]\n",
    ))
    .unwrap();
    let mut saved = Simulation::new();
    saved
        .session_mut_for_test()
        .set_map_name_for_test("LOCAL-OWNER.MAP".into());
    saved.session_mut_for_test().set_map_width_for_test(8);
    saved.session_mut_for_test().set_map_height_for_test(8);
    *saved.overlay_grid_mut_for_test() = Some(OverlayGrid::new(8, 8));
    saved.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(8, 8, Vec::new()));
    let outgoing = saved.intern("New Player");
    let player = saved.intern("Player");
    for (index, owner) in [outgoing, player].into_iter().enumerate() {
        // Both human, with the outgoing owner registered first: neither
        // is_human nor House registration order identifies the saved viewer.
        saved.houses_mut_for_test().register(HouseState::new(
            owner,
            index as u8,
            None,
            true,
            0,
            10,
        ));
    }
    saved
        .session_mut_for_test()
        .set_current_house_for_test(Some(player));
    saved.fog_mut_for_test().set_width_for_test(8);
    saved.fog_mut_for_test().set_height_for_test(8);
    saved
        .fog_mut_for_test()
        .reveal_cells_for_owner(player, [(3, 4)]);
    assert!(saved.fog().is_cell_revealed(player, 3, 4));
    assert!(!saved.fog().is_cell_revealed(outgoing, 3, 4));

    let directory = std::env::temp_dir().join(format!("vera-local-owner-{}", std::process::id()));
    let repository = SaveRepository::at(&directory);
    let path = repository
        .write_named(
            "saved-player.bin",
            &GameSnapshot::save_validated(&saved, 91, rules.simulation_config_hash(), "owner", 1),
        )
        .unwrap();
    let mut runtime = SimRuntime::from_simulation(saved);
    runtime
        .simulation
        .session_mut_for_test()
        .set_current_house_for_test(Some(outgoing));
    runtime.resources_mut().rules = rules;
    runtime.resources_mut().overlay_registry = OverlayTypeRegistry::empty();
    runtime.resources_mut().terrain_template =
        Some(ResolvedTerrainGrid::from_cells(8, 8, Vec::new()));
    assert_eq!(
        scenario_local_owner(&runtime.simulation),
        Some("New Player")
    );

    let prepared = PreparedLoad::from_repository(
        LoadPreparationView::from_runtime(&repository, Some(&runtime), Some(91)),
        &path,
    )
    .unwrap();
    prepared.commit_into(&mut runtime);
    std::fs::remove_dir_all(directory).unwrap();
    assert_eq!(runtime.simulation.session().current_house(), Some(player));
    let owner = scenario_local_owner(&runtime.simulation)
        .unwrap()
        .to_owned();
    assert_eq!(
        owner, "Player",
        "the outgoing identity must not survive saved current House replacement"
    );
    let sim = &mut runtime.simulation;
    assert!(sim.prepare_fog_view_for(&owner));
    let viewer = sim.interner().get(&owner).unwrap();
    assert!(sim.fog().is_cell_revealed(viewer, 3, 4));
    assert!(!sim.fog().is_cell_revealed(outgoing, 3, 4));
    assert!(schedule_command_in_sim(sim, &owner, Command::SetGameSpeed { speed: 4 }).is_some());
    assert_eq!(sim.pending_command_snapshot()[0].owner, player);
}
