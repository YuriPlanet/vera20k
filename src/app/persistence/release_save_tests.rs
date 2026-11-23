//! Restore files from the released stock desktop writers through the same
//! resource, view and world checks that precede GPU allocation in both menus.

use super::*;
use crate::map::source::test_support::TestDirectory;
use crate::rules::ini_parser::IniFile;
use crate::sim::scenario_session::ScenarioGameMode;
use crate::sim::snapshot::scenario_resources::SavedScenarioResourcesRef;

// One libtest case per release so a broken old reader does not hide failures
// for later releases. The newest fixture also identifies the current writer.
macro_rules! released_saves {
    ($( $case:ident: $tag:literal, $layout:literal, $mode:ident; )+) => {
        const RELEASES: &[&str] = &[$($tag),+];
        $(
            #[test]
            fn $case() {
                crate::test_fixture::on_game_stack(|| {
                    resume_and_save_again($tag, $layout, ScenarioGameMode::$mode)
                });
            }
        )+
    };
}

released_saves! {
    v0_3_8: "v0.3.8", 0xFD30_A463, Skirmish;
    v0_3_9: "v0.3.9", 0xFD30_A463, Campaign;
    v0_4_0: "v0.4.0", 0x9BB2_0D64, Skirmish;
    main_eb05f451e: "main-eb05f451e", 0xFD54_FFFF, Skirmish;
    main_41f140f93: "main-41f140f93", 0xCB54_5137, Skirmish;
    main_6d6caa108: "main-6d6caa108", 0xCADF_6067, Skirmish;
    main_ffe8b25af: "main-ffe8b25af", 0xD6C7_5D57, Skirmish;
}

const TUNNEL_FIXTURE: &str = "v0.3.9-tunnel";
const ALPHA_FIXTURE: &str = "v0.4.0-alpha";
const SEQUENCE_FIXTURE: &str = "v0.4.0-sequence";
const IMAGE_FIXTURE: &str = "main-eb05f451e-images";

#[test]
fn every_frozen_save_has_a_load_case() {
    use std::collections::BTreeSet;
    let directory = crate::test_fixture::checkout_root().join("tools/save_compatibility/fixtures");
    let frozen: BTreeSet<_> = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "sav"))
        .map(|path| path.file_stem().unwrap().to_str().unwrap().to_owned())
        .collect();
    let cases: BTreeSet<_> = RELEASES
        .iter()
        .copied()
        .chain([
            TUNNEL_FIXTURE,
            ALPHA_FIXTURE,
            SEQUENCE_FIXTURE,
            IMAGE_FIXTURE,
        ])
        .map(str::to_owned)
        .collect();
    assert_eq!(
        cases, frozen,
        "every frozen player save needs a load and resume test"
    );
}

fn fixture(release: &str) -> Vec<u8> {
    let file_name = format!("{release}.sav");
    let path = crate::test_fixture::checkout_root()
        .join("tools/save_compatibility/fixtures")
        .join(&file_name);
    let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let sums = crate::test_fixture::text("tools/save_compatibility/fixtures/SHA256SUMS");
    let digest = sums
        .lines()
        .filter_map(|line| line.split_once("  "))
        .find(|(_, name)| *name == file_name)
        .unwrap_or_else(|| panic!("missing source digest for {file_name}"))
        .0;
    assert_eq!(
        crate::util::sha256::sha256_hex(&bytes),
        digest,
        "{file_name}"
    );
    bytes
}

#[test]
fn current_writer_has_a_frozen_player_save() {
    let latest = RELEASES.last().unwrap();
    let header = GameSnapshot::read_metadata(&fixture(latest)).unwrap();
    assert!(
        header.is_compatible(),
        "the current writer changed; add its frozen save and keep every older row"
    );
    // A stock snapshot-layout build has no test-support. With its output set,
    // a cfg-only change to the actual player's wire layout fails this row.
    if let Ok(stock) = std::env::var("VERA20K_STOCK_WRITER_LAYOUT") {
        assert_eq!(
            header.layout,
            stock.parse::<u32>().unwrap(),
            "stock writer layout"
        );
    }
}

/// A loose, redistributable temperate theater with one clear ground tile.
fn theater_assets(directory: &TestDirectory) -> AssetManager {
    directory.write(
        "COMPATGLOW.SHP",
        crate::test_fixture::bytes("tools/save_compatibility/writer/COMPATGLOW.SHP"),
    );
    directory.write(
        "temperatmd.ini",
        b"[TileSet0000]\nFileName=clear\nTilesInSet=1\n",
    );
    for palette in ["isotem.pal", "unittem.pal", "temperat.pal"] {
        directory.write(palette, &[0; 768]);
    }
    // One 8 x 4 TMP diamond, image block at byte 20, zero height and clear land.
    let mut tile = Vec::new();
    for word in [1u32, 1, 8, 4, 20] {
        tile.extend_from_slice(&word.to_le_bytes());
    }
    tile.resize(20 + 52 + 16, 0);
    directory.write("clear01.tem", &tile);
    AssetManager::from_loose_root_for_test(directory.path())
}

fn prepare(
    repository: &SaveRepository,
    path: &std::path::Path,
    assets: &mut AssetManager,
    owner: &NativeRulesProcessOwner,
    main: &MainRng,
) -> PreparedSavedResources {
    let bytes = repository.read(path).unwrap();
    let view = SavedGamePreparationView {
        repository,
        current_simulation: None,
        frontend_main: main,
        rules_owner: owner,
        tile_cache: &TileVariantSelectorCache::default(),
        lighting: &crate::app::presentation::lighting::MatchLighting::default(),
        load_clock: &|| 1000,
    };
    assets
        .prepare_saved_load(|assets| PreparedSavedGame::prepare_resources(&bytes, &view, assets))
        .unwrap_or_else(|error| panic!("restoring {}: {error}", path.display()))
}

fn fixture_owner() -> NativeRulesProcessOwner {
    NativeRulesProcessOwner::from_cold_start_sources(
        IniFile::from_str("[VehicleTypes]\n0=APC\n[APC]\nStrength=100\nSpeed=4\n"),
        None,
        IniFile::from_str(""),
        Default::default(),
        Default::default(),
    )
    .unwrap()
}

fn exercise(
    name: &str,
    layout: u32,
    advance: impl FnOnce(&mut SimRuntime),
    verify: impl Fn(&Simulation),
) {
    let original = fixture(name);
    let header = GameSnapshot::read_metadata(&original).unwrap();
    assert_eq!(
        (header.envelope_version, header.layout),
        (2, layout),
        "{name}"
    );
    let directory = TestDirectory::new(name);
    let mut assets = theater_assets(&directory);
    let repository = SaveRepository::at(directory.path().join("saves"));
    let source = repository.write_named("old.sav", &original).unwrap();
    resume_player_file(
        &repository,
        &source,
        &mut assets,
        &mut fixture_owner(),
        &MainRng::new(1),
        advance,
        verify,
    );
}

fn resume_and_save_again(release: &str, layout: u32, mode: ScenarioGameMode) {
    let check_world = |sim: &Simulation| {
        assert!(sim.alpha_shape_slots_for_test().is_empty(), "{release}");
        assert_eq!(sim.session().map_name(), "compat.map", "{release}");
        assert_eq!(sim.session().game_mode(), mode, "{release}");
        let player = sim.session().current_house().expect("fixture has a player");
        assert_eq!(sim.interner().resolve(player), "Americans", "{release}");
    };
    exercise(
        release,
        layout,
        |runtime| {
            check_world(&runtime.simulation);
            assert_eq!(runtime.resources.rules.object("APC").unwrap().image, "APC");
            assert_eq!(runtime.simulation.session().tick(), 91, "{release}");
            assert_eq!(
                runtime.simulation.session().game_options().game_speed,
                1,
                "{release}"
            );
            let commands = runtime.simulation.take_due_commands();
            runtime.advance_frame_for_tooling(&commands, 67).unwrap();
            assert_eq!(
                runtime.simulation.session().game_options().game_speed,
                4,
                "{release}"
            );
        },
        |sim| {
            check_world(sim);
            assert_eq!(sim.session().tick(), 92, "{release}");
            assert_eq!(sim.session().game_options().game_speed, 4, "{release}");
        },
    );
}

#[test]
fn released_animation_sequence_rules_resume_and_save_with_the_current_hash() {
    crate::test_fixture::on_game_stack(|| {
        resume_changed_digest(SEQUENCE_FIXTURE, 0x9BB2_0D64, |rules| {
            assert!(rules.animation_sequence("POST").is_some());
        });
    });
}

#[test]
fn historical_anim_and_overlay_image_rules_resume_and_save_with_the_current_hash() {
    crate::test_fixture::on_game_stack(|| {
        resume_changed_digest(IMAGE_FIXTURE, 0xFD54_FFFF, |rules| {
            assert_eq!(rules.anim_type_name_by_index(0), Some("COMPATGLOW"));
            assert_eq!(rules.overlay_registry().name(0), Some("POST"));
        });
    });
}

fn resume_changed_digest(name: &str, layout: u32, check: impl Fn(&crate::rules::ruleset::RuleSet)) {
    let saved_hash = GameSnapshot::read_metadata(&fixture(name))
        .unwrap()
        .rules_hash;
    exercise(
        name,
        layout,
        |runtime| {
            let rules = &runtime.resources.rules;
            check(rules);
            assert_ne!(saved_hash, rules.simulation_config_hash());
            let commands = runtime.simulation.take_due_commands();
            assert_eq!(commands.len(), 1);
            runtime.advance_frame_for_tooling(&commands, 67).unwrap();
        },
        |sim| {
            assert_eq!(sim.session().tick(), 92);
            assert_eq!(sim.session().game_options().game_speed, 4);
        },
    );
}

#[test]
fn released_alpha_shapes_retain_their_owners_and_purge_only_the_expired_glow() {
    crate::test_fixture::on_game_stack(|| {
        let remaining = |sim: &Simulation| {
            assert_eq!(
                sim.alpha_shape_slots_for_test(),
                [(2, [-31, 44], "COMPATGLOW", false, false)]
            );
        };
        exercise(
            ALPHA_FIXTURE,
            0x9BB2_0D64,
            |runtime| {
                assert_eq!(
                    runtime.simulation.alpha_shape_slots_for_test(),
                    [
                        (1, [29, 44], "COMPATGLOW", true, false),
                        (2, [-31, 44], "COMPATGLOW", false, false),
                    ]
                );
                let commands = runtime.simulation.take_due_commands();
                runtime.advance_frame_for_tooling(&commands, 67).unwrap();
                remaining(&runtime.simulation);
            },
            remaining,
        );
    });
}

#[test]
fn released_transit_reaches_one_exit_and_saves_again() {
    crate::test_fixture::on_game_stack(check_released_transit);
}

fn check_released_transit() {
    let check_exit = |sim: &Simulation| {
        let unit = sim.entities().get(1).expect("saved APC");
        assert!(unit.active_tube().is_none());
        assert_eq!((unit.position.rx(), unit.position.ry()), (2, 0));
        let exit = sim.occupancy().get(2, 0).expect("APC occupies the exit");
        assert_eq!(
            exit.occupants
                .iter()
                .filter(|object| object.entity_id == 1)
                .count(),
            1,
            "the exit must contain exactly one membership for the APC"
        );
        assert!(!sim.occupancy().contains_entity(0, 0, 1));
    };
    exercise(
        TUNNEL_FIXTURE,
        0xFD30_A463,
        |runtime| {
            assert!(
                runtime
                    .simulation
                    .entities()
                    .get(1)
                    .unwrap()
                    .active_tube()
                    .is_some()
            );
            for _ in 0..400 {
                let commands = runtime.simulation.take_due_commands();
                runtime.advance_frame_for_tooling(&commands, 67).unwrap();
                if runtime
                    .simulation
                    .entities()
                    .get(1)
                    .unwrap()
                    .active_tube()
                    .is_none()
                {
                    check_exit(&runtime.simulation);
                    return;
                }
            }
            panic!("saved APC never emerged from the tunnel");
        },
        check_exit,
    );
}

pub(super) fn resume_player_file(
    repository: &SaveRepository,
    source: &std::path::Path,
    assets: &mut AssetManager,
    owner: &mut NativeRulesProcessOwner,
    main: &MainRng,
    advance: impl FnOnce(&mut SimRuntime),
    verify: impl Fn(&Simulation),
) {
    let original = repository.read(source).unwrap();
    let header = GameSnapshot::read_metadata(&original).unwrap();
    let path = if header.is_compatible() {
        source.to_owned()
    } else {
        assert!(
            SaveRepository::can_migrate(&header),
            "{} is unavailable in the load menu",
            source.display()
        );
        let name = " 战役 / 续玩? ";
        let path = repository.migrate_to_slot(source, name, None).unwrap();
        assert_eq!(path.parent(), Some(repository.directory()));
        let filename = path.file_name().unwrap().to_str().unwrap();
        let timestamp = filename
            .strip_prefix(&format!("save_战役 _ 续玩__tick{}_", header.tick))
            .and_then(|name| name.strip_suffix(".bin"))
            .expect("ordinary new save filename");
        assert!(timestamp.parse::<u64>().is_ok());
        assert!(
            repository.read(source).unwrap() == original,
            "source changed during conversion"
        );
        let mut backups = std::fs::read_dir(repository.directory().join("backups")).unwrap();
        let backup = backups.next().unwrap().unwrap().path();
        assert!(backups.next().is_none());
        assert!(
            repository.read(&backup).unwrap() == original,
            "backup differs from source"
        );
        let converted = repository.read(&path).unwrap();
        let converted_header = GameSnapshot::read_header(&converted).unwrap();
        assert_eq!(converted_header.description, name);
        if matches!(header.layout, 0xFD30_A463 | 0x9BB2_0D64 | 0xFD54_FFFF) {
            // Both newly saved flags default to false in historical worlds.
            // Every original body byte survives in order, including any
            // animation-listener flag already present in the eb05f451e layout.
            let original_body =
                &original[crate::util::save_codec::serialize(&header).unwrap().len()..];
            let converted_body =
                &converted[crate::util::save_codec::serialize(&converted_header)
                    .unwrap()
                    .len()..];
            let mut source = original_body.iter().copied().peekable();
            for byte in converted_body {
                if source.next_if_eq(byte).is_none() {
                    assert_eq!(*byte, 0, "conversion changed the historical world");
                }
            }
            assert!(source.next().is_none(), "conversion truncated the world");
        }
        assert!(
            repository
                .browser_entries()
                .iter()
                .any(|(row, _)| { row.path == path && row.header.description == name })
        );
        path
    };
    let mut loaded = prepare(repository, &path, assets, owner, main);
    let dummy = SharedCellDummy::fresh();
    let selector = loaded.tile_cache.selector();
    let (mut simulation, presented) = loaded.world.into_committed(dummy.clone(), main);
    simulation.bind_tile_variant_selector(selector.clone());
    let template = loaded.resources.terrain_template.as_mut().unwrap();
    template.bind_shared_cell_dummy(dummy);
    template.bind_tile_variant_selector(selector);
    let mut runtime = SimRuntime {
        simulation,
        resources: loaded.resources.into(),
    };
    let mut input = crate::app::input::state::MatchInputState::new(Default::default());
    let mut gadgets = Default::default();
    loaded.local_view.commit_into(&mut input, &mut gadgets);
    let tab = crate::app::sidebar_projection::loaded_active_tab(presented.sidebar_order.as_ref());
    let mut sidebar = crate::app::sidebar_projection::SidebarProjectionState::default();
    sidebar.restore_order(presented.sidebar_order);
    let terrain = runtime.simulation.resolved_terrain().unwrap();
    let mut grid = crate::app::presentation::lighting::rebuild_lighting_grid_from_sim(
        terrain,
        loaded.map.lighting(),
        Some(&runtime.simulation),
        2,
    );
    grid.stage_saved_local_lighting(&loaded.lighting, loaded.scheme_normal_rgb);
    let mut lighting = crate::app::presentation::lighting::MatchLighting::default();
    lighting.install(
        grid,
        loaded.map.lighting().clone(),
        2,
        Some((terrain, &runtime.simulation)),
        None,
    );
    lighting.restore_anim_intensities(presented.anim_intensities);
    advance(&mut runtime);
    let sim = &runtime.simulation;
    let mut view = input
        .capture_saved_local_view(sim, &runtime.resources.rules, &gadgets)
        .unwrap();
    view.set_power_bar(loaded.power_bar_anim.saved());
    view.set_credits_counter(loaded.credits_counter);
    view.set_radar_housing(loaded.radar_housing);
    view.set_radar_events(loaded.radar_events.saved());
    let (cell_lighting, anim_intensities) = lighting.saved_at(sim);
    view.set_lighting(cell_lighting);
    let terrain = runtime
        .resources
        .terrain_template
        .as_ref()
        .unwrap()
        .capture_user_saved_template(sim.resolved_terrain().unwrap())
        .unwrap();
    let resources = SavedScenarioResourcesRef::capture(
        &runtime.resources,
        &loaded.processed_rules,
        owner.fixed_art().content_hash(),
        terrain,
        loaded.map,
    );
    let order = sidebar
        .saved_order()
        .map(|saved| saved.with_active_tab(tab.tab_index() as u8));
    let bytes = GameSnapshot::save_validated_with_presentation(
        sim,
        header.map_hash,
        runtime.resources.rules.simulation_config_hash(),
        "resumed",
        123,
        &presented.sinking_waterlines,
        &anim_intensities,
        order.as_ref(),
        presented.tactical_view.as_ref(),
        Some(&view),
        Some(&resources),
    )
    .unwrap();
    assert!(GameSnapshot::read_header(&bytes).unwrap().is_compatible());
    let path = repository.write_named("resumed.sav", &bytes).unwrap();
    owner.install_saved_scenario_rules(loaded.processed_rules);
    verify(
        &prepare(repository, &path, assets, owner, main)
            .world
            .simulation,
    );
}
