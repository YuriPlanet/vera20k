//! App identity after the real prepare/commit load transaction, without GPU state.
use super::{preferred_local_owner_for_sim, schedule_command_in_sim};
use crate::app::persistence::{LoadPreparationView, PreparedLoad, SaveRepository};
use crate::map::houses::HouseRoster;
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
        .session
        .set_map_name_for_test("LOCAL-OWNER.MAP".into());
    saved.session.set_map_width_for_test(8);
    saved.session.set_map_height_for_test(8);
    saved.overlay_grid = Some(OverlayGrid::new(8, 8));
    saved.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(8, 8, Vec::new()));
    let outgoing = saved.interner.intern("New Player");
    let player = saved.interner.intern("Player");
    for (index, owner) in [outgoing, player].into_iter().enumerate() {
        // Both human, with the outgoing owner registered first: neither
        // is_human nor House registration order identifies the saved viewer.
        saved
            .houses
            .register(HouseState::new(owner, index as u8, None, true, 0, 10));
    }
    saved.session.set_current_house_for_test(Some(player));
    saved.fog.width = 8;
    saved.fog.height = 8;
    saved.fog.reveal_cells_for_owner(player, [(3, 4)]);
    assert!(saved.fog.is_cell_revealed(player, 3, 4));
    assert!(!saved.fog.is_cell_revealed(outgoing, 3, 4));

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
        .session
        .set_current_house_for_test(Some(outgoing));
    runtime.resources.rules = rules;
    runtime.resources.overlay_registry = OverlayTypeRegistry::empty();
    runtime.resources.terrain_template = Some(ResolvedTerrainGrid::from_cells(8, 8, Vec::new()));
    let outgoing_app_owner = "New Player";
    let roster = HouseRoster::default();
    assert_eq!(
        preferred_local_owner_for_sim(
            &runtime.simulation,
            Some(&runtime.resources.rules),
            &roster,
            Some(outgoing_app_owner),
        )
        .as_deref(),
        Some(outgoing_app_owner)
    );

    let prepared = PreparedLoad::from_repository(
        LoadPreparationView::from_runtime(&repository, Some(&runtime), Some(91)),
        &path,
    )
    .unwrap();
    prepared.commit_into(&mut runtime);
    std::fs::remove_dir_all(directory).unwrap();
    assert_eq!(runtime.simulation.session.current_house(), Some(player));
    let owner = preferred_local_owner_for_sim(
        &runtime.simulation,
        Some(&runtime.resources.rules),
        &roster,
        Some(outgoing_app_owner),
    )
    .unwrap();
    assert_eq!(
        owner, "Player",
        "the outgoing app identity must not survive saved current House replacement"
    );
    let sim = &mut runtime.simulation;
    assert!(sim.prepare_fog_view_for(&owner));
    let viewer = sim.interner.get(&owner).unwrap();
    assert!(sim.fog.is_cell_revealed(viewer, 3, 4));
    assert!(!sim.fog.is_cell_revealed(outgoing, 3, 4));
    assert!(schedule_command_in_sim(sim, &owner, Command::SetGameSpeed { speed: 4 }).is_some());
    assert_eq!(sim.pending_command_snapshot()[0].owner, player);
}

#[test]
fn current_house_excludes_selection_and_debug_override_but_sandbox_keeps_both() {
    use crate::map::entities::EntityCategory;
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;

    let roster = HouseRoster::default();
    let mut sim = Simulation::new();
    let player = sim.interner.intern("Player");
    let selected = sim.interner.intern("SelectedHouse");
    let type_ref = sim.interner.intern("TESTUNIT");
    sim.houses
        .register(HouseState::new(player, 0, None, true, 0, 10));
    assert_eq!(
        preferred_local_owner_for_sim(&sim, None, &roster, Some("SandboxHouse")).as_deref(),
        Some("SandboxHouse"),
        "identity-less development worlds retain the explicit preference"
    );
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        42,
        1,
        1,
        0,
        0,
        selected,
        Health::new(100),
        type_ref,
        EntityCategory::Unit,
        0,
        5,
        false,
    );
    entity.selection.set_selected_for_test(true);
    sim.substrate.entities.insert(entity);
    assert_eq!(
        preferred_local_owner_for_sim(&sim, None, &roster, Some("SandboxHouse")).as_deref(),
        Some("SelectedHouse"),
        "sandbox selection preserves the existing precedence over its override"
    );
    sim.session.set_current_house_for_test(Some(player));
    assert_eq!(
        preferred_local_owner_for_sim(&sim, None, &roster, Some("SandboxHouse")).as_deref(),
        Some("Player"),
        "ordinary identity cannot follow selection or a retained debug preference"
    );
}
