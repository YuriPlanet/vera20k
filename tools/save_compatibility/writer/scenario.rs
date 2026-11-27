use crate::map::resolved_terrain::{ResolvedTerrainGrid, SharedCellDummy};
use crate::rules::ini_parser::IniFile;
use crate::rules::process_owner::{NativeRulesProcessOwner, NativeScenarioRulesPrefix};
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::house_state::HouseState;
use crate::sim::runtime::SimResources;
use crate::sim::scenario_session::ScenarioGameMode;
use crate::sim::snapshot::saved_local_view::{SavedLocalView, SavedPowerBar};
use crate::sim::snapshot::scenario_resources::{SavedMapMetadata, SavedScenarioResourcesRef};
use crate::sim::snapshot::{GameSnapshot, PlayerSaveCells};
use crate::sim::world::{Simulation, SnapshotProcessState};

fn bind(rules: &mut crate::rules::ruleset::RuleSet, art: &IniFile) {
    let root = std::path::Path::new("target/save-fixture-writer/assets");
    let assets = crate::assets::asset_manager::AssetManager::from_loose_root_for_test(root);
    crate::bindings::bind_rules_art_metadata(rules, art, &assets, "tem", "TEMPERATE");
    crate::bindings::bind_rules_asset_closure(
        rules,
        &[],
        &[],
        &assets,
        "tem",
        "TEMPERATE",
        crate::rules::terrain_asset_catalog::TerrainAssetLoadPhase::Saved,
    )
    .unwrap();
}

pub struct Scenario {
    pub ini: &'static str,
    pub populate: fn(&mut Simulation, &crate::rules::ruleset::RuleSet, &IniFile),
    pub verify: fn(&Simulation, bool),
}

pub const STANDARD: Scenario = Scenario {
    ini: "[Map]\nTheater=TEMPERATE\n[Basic]\nName=Compatibility\n",
    populate: |_, _, _| {},
    verify: |_, _| {},
};

pub const SEQUENCES: Scenario = Scenario {
    ini: "[Map]\nTheater=TEMPERATE\n[Basic]\nName=Compatibility\n\
          [BuildingTypes]\n0=POST\n[POST]\nStrength=100\n",
    populate: |_, rules, _| {
        assert!(rules.animation_sequence("POST").is_some());
        println!(
            "historical rules hash {:016x}",
            rules.simulation_config_hash()
        );
    },
    ..STANDARD
};

pub const IMAGE_STATES: Scenario = Scenario {
    ini: "[Map]\nTheater=TEMPERATE\n[Basic]\nName=Compatibility\n\
          [Animations]\n0=COMPATGLOW\n[OverlayTypes]\n0=POST\n[POST]\nImage=COMPATGLOW\n",
    populate: |_, rules, _| {
        assert_eq!(rules.anim_type_name_by_index(0), Some("COMPATGLOW"));
        assert_eq!(rules.overlay_registry().name(0), Some("POST"));
        println!(
            "historical rules hash {:016x}",
            rules.simulation_config_hash()
        );
    },
    ..STANDARD
};

pub fn write_world(
    mut sim: Simulation,
    template: ResolvedTerrainGrid,
    path: &std::path::Path,
    mode: ScenarioGameMode,
    scenario: Scenario,
) {
    let base = IniFile::from_str("[VehicleTypes]\n0=APC\n[APC]\nStrength=100\nSpeed=4\n");
    let map_ini = IniFile::from_str(scenario.ini);
    let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
        base,
        None,
        IniFile::from_str(""),
        Default::default(),
        Default::default(),
    )
    .unwrap();
    let prefix = if mode == ScenarioGameMode::Campaign {
        NativeScenarioRulesPrefix::Campaign(None)
    } else {
        NativeScenarioRulesPrefix::NonCampaign(None)
    };
    let (mut rules, _, _, _) = owner.load_scenario(prefix, &map_ini).unwrap().into_parts();
    bind(&mut rules, owner.fixed_art());
    let mut resources = SimResources::new(rules);
    let name = "compat.map";
    sim.install_session_mode(mode);
    let house = sim.intern("Americans");
    if sim.session().current_house().is_none() {
        let mut state = HouseState::new(house, 0, None, true, 1000, 10);
        state.set_player_control_for_test(true);
        sim.register_house(state);
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(house));
    }
    let (width, height) = (template.width(), template.height());
    sim.session_mut_for_test()
        .set_map_name_for_test(name.to_owned());
    sim.session_mut_for_test()
        .set_theater_for_test("TEMPERATE".to_owned());
    sim.session_mut_for_test().set_map_width_for_test(width);
    sim.session_mut_for_test().set_map_height_for_test(height);
    sim.session_mut_for_test().set_tick_for_test(91);
    sim.session_mut_for_test().set_binary_frame_for_test(92);
    *sim.overlay_grid_mut_for_test() =
        Some(crate::sim::overlay_grid::OverlayGrid::new(width, height));
    if sim.resolved_terrain().is_none() {
        sim.install_resolved_terrain_for_new_map(template.clone());
    }
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    (scenario.populate)(&mut sim, &resources.rules, &map_ini);
    sim.queue_command(CommandEnvelope::new(
        house,
        92,
        Command::SetGameSpeed { speed: 4 },
    ));
    sim.rebuild_dynamic_navigation(&resources.rules);
    let map_hash = map_ini.content_hash();
    let map = SavedMapMetadata::capture(
        map_hash,
        &crate::map::source::LoadedMapSource::LegacyFallback { label: name.into() },
        &Default::default(),
        &Default::default(),
        &Default::default(),
        &[],
        &Default::default(),
        &Default::default(),
        &Default::default(),
        0,
        None,
        [-1, -1],
        false,
    );
    let saved_template = template.capture_saved_template().unwrap();
    resources.terrain_template = Some(template);
    let mut saved = SavedScenarioResourcesRef::capture(
        &resources,
        owner.saved_scenario_rules().unwrap(),
        owner.fixed_art().content_hash(),
        saved_template,
        map,
    );
    let live = sim.resolved_terrain().unwrap();
    saved.capture_live_cell_land(live).unwrap();
    let mut view =
        SavedLocalView::new([[0, 0]; 4], Vec::new(), false, None, Default::default()).unwrap();
    view.set_power_bar(SavedPowerBar {
        counts_surplus_output_drain: [0; 3],
        cached_output: 0,
        cached_drain: 0,
        flashes_remaining: 0,
        flash_timer: [-1, 0],
        animating: false,
    });
    view.set_radar_events(Default::default());
    view.set_lighting(crate::map::lighting::SavedCellLighting {
        cells: live.iter().map(|c| ((c.rx, c.ry), 1000, 0, 1000)).collect(),
    });
    let bytes = GameSnapshot::save_validated_with_presentation(
        &sim,
        map_hash,
        resources.rules.simulation_config_hash(),
        concat!("Compatibility v", env!("CARGO_PKG_VERSION")),
        123,
        &[],
        &[],
        None,
        None,
        Some(&view),
        Some(&saved),
    )
    .unwrap();
    let mut snapshot = GameSnapshot::load_with_resources(&bytes).unwrap();
    let saved = snapshot.take_scenario_resources().unwrap();
    let land = saved.terrain().user_load_land_values().unwrap();
    let mut rules = owner
        .project_saved_scenario_rules(saved.processed_rules())
        .unwrap();
    bind(&mut rules, owner.fixed_art());
    assert_eq!(rules.simulation_config_hash(), snapshot.rules_hash);
    let (mut restored_resources, _, _) = saved
        .into_restored_parts(rules, SharedCellDummy::fresh())
        .unwrap();
    let (mut restored, _) = snapshot
        .restore_player_world(
            SnapshotProcessState::Live(&sim),
            &restored_resources.rules,
            restored_resources.terrain_template.take().unwrap(),
            Default::default(),
            PlayerSaveCells::Saved {
                land,
                prepare_assets: &mut |_, _, _| Ok(()),
            },
        )
        .unwrap();
    assert_eq!(restored.session().current_house(), Some(house));
    assert_eq!(restored.session().tick(), 91);
    (scenario.verify)(&restored, false);
    let due = restored.take_due_commands();
    let registry = restored_resources.rules.overlay_registry();
    let result = restored.advance_tick(
        &due,
        Some(&restored_resources.rules),
        None,
        Some(registry),
        67,
    );
    assert_eq!(result.executed_commands, 1);
    assert_eq!(restored.session().game_options().game_speed, 4);
    (scenario.verify)(&restored, true);
    use std::io::Write;
    std::fs::File::create_new(path)
        .unwrap()
        .write_all(&bytes)
        .unwrap();
}
pub fn write(path: &std::path::Path, campaign: bool, scenario: Scenario) {
    write_world(
        Simulation::with_seed(17),
        crate::map::resolved_terrain::test_flat_ground_grid(3),
        path,
        if campaign {
            ScenarioGameMode::Campaign
        } else {
            ScenarioGameMode::Skirmish
        },
        scenario,
    );
}
