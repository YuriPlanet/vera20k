//! Retail campaign loading and Mission_Harvest with controlled resource/fog inputs.
use super::*;

struct SilentProgress;
impl LoadingProgressSink for SilentProgress {
    fn milestone(&mut self, _percent: u32) {}
}

/// Preserve the retail terrain, ore packs, rules and roster. Select its two
/// authored CMINs and refinery and remove mission scripts/other actors so the
/// reproduction controls mining instead of advancing ALL05's battle script.
/// An owner control moves the tested miner and its required refinery together.
fn controlled_all05(bytes: &[u8], miner_owner: Option<&str>) -> Vec<u8> {
    let ini = crate::rules::ini_parser::IniFile::from_bytes(bytes).unwrap();
    let mut output = String::new();
    for name in ini.section_names() {
        output.push_str(&format!("[{name}]\n"));
        for (key, value) in ini.section(name).unwrap().raw_entries() {
            let keep = match name {
                "Units" => matches!(key, "4" | "5"),
                "Structures" => key == "1030379",
                "Infantry"
                | "Aircraft"
                | "Triggers"
                | "Events"
                | "Actions"
                | "Tags"
                | "CellTags"
                | "TeamTypes"
                | "TaskForces"
                | "ScriptTypes"
                | "Scripts"
                | "AITriggerTypes"
                | "AITriggerTypesEnable" => false,
                _ => true,
            };
            if keep {
                if ((name == "Units" && key == "4") || (name == "Structures" && key == "1030379"))
                    && let Some(owner) = miner_owner
                {
                    let (_, rest) = value.split_once(',').unwrap();
                    output.push_str(&format!("{key}={owner},{rest}\n"));
                } else {
                    output.push_str(&format!("{key}={value}\n"));
                }
            }
        }
    }
    // Retail IniFile widens input bytes; reverse that widening rather than
    // turning retained high bytes into a second UTF-8 encoding.
    output
        .chars()
        .map(|ch| u8::try_from(u32::from(ch)).unwrap())
        .collect()
}

fn load_campaign(
    root: &Path,
    assets: &mut AssetManager,
    directory: &Path,
    gpu: &GpuContext,
    batch: &BatchRenderer,
) -> crate::sim::runtime::SimRuntime {
    use crate::app::match_bootstrap::{CampaignLaunchOptions, prepare_campaign_startup};
    let mut process = crate::app::process_assets::ProcessAssets::new(
        crate::assets::asset_manager::MediaArchiveMode::STOCK_DIGITAL,
        false,
    );
    process.initialize_sources_if_needed(assets).unwrap();
    let startup = prepare_campaign_startup(
        process.campaigns().unwrap(),
        "ALL05",
        1,
        Some("ALL05UMD.MAP"),
        Some(CampaignLaunchOptions {
            game_speed: 3,
            build_off_ally: false,
            scenario_directory: Some(directory.to_owned()),
        }),
        &mut TestClock(31),
    )
    .unwrap();
    let request = LoadingRequest::campaign(startup);
    let initial = request
        .load_initial_with_assets(
            root.to_owned(),
            assets,
            process.native_rules(),
            &mut SilentProgress,
        )
        .unwrap();
    let PreparedScenarioLoad {
        request,
        initial,
        context,
    } = request.prepare_initial(initial).unwrap();
    let dummy = process.shared_cell_dummy.clone();
    let (rules, cache, audio) = process.native_rules_mut_with_tile_cache();
    let mut result = init::load_map_from_initial(
        gpu,
        batch,
        assets,
        initial,
        request.startup,
        context,
        false,
        8,
        rules.unwrap(),
        audio,
        dummy,
        cache,
        &mut SilentProgress,
    )
    .unwrap();
    crate::sim::runtime::SimRuntime {
        simulation: result.scenario.simulation.take().unwrap(),
        resources: crate::sim::runtime::SimResources {
            rules: result.scenario.rules.take().unwrap(),
            overlay_registry: result.scenario.overlay_registry,
            terrain_template: result.scenario.resolved_terrain.take(),
        },
    }
}

/// Drive real frames until the map-authored Harvest mission has dispatched.
fn dispatch(runtime: &mut crate::sim::runtime::SimRuntime, miner: u64) -> Option<(u16, u16)> {
    use crate::sim::components::NavTargetRef;
    runtime.simulation.session_mut_for_test().publish_started();
    for _ in 0..8 {
        runtime
            .advance_frame(
                &[],
                67,
                crate::sim::world::TickLane::Ordinary,
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap();
        let entity = runtime.simulation.entities().get(miner).unwrap();
        if entity.mission.dispatch_timer().delay() != 0 {
            return match entity.navigation.nav_com() {
                Some(NavTargetRef::Cell { rx, ry }) => Some((rx, ry)),
                None => None,
                Some(other) => panic!("unexpected mining destination {other:?}"),
            };
        }
    }
    panic!("authored Harvest mission did not dispatch");
}

fn miner(runtime: &crate::sim::runtime::SimRuntime) -> u64 {
    let sim = &runtime.simulation;
    let current = sim.session().current_house().unwrap();
    let cmin = sim.interner().get("CMIN").unwrap();
    let id = sim
        .entities()
        .values()
        .find(|e| e.type_ref() == cmin && (e.position.rx(), e.position.ry()) == (52, 93))
        .unwrap()
        .stable_id();
    assert_eq!(
        sim.entities()
            .get(id)
            .unwrap()
            .discovery
            .owned_by_current_house(),
        sim.entities().get(id).unwrap().owner() == current,
        "the actual campaign constructor must classify the player miner"
    );
    id
}

fn keep_ore(runtime: &mut crate::sim::runtime::SimRuntime, keep: impl Fn(u16, u16) -> bool) {
    let sim = &mut runtime.simulation;
    let registry = &runtime.resources.overlay_registry;
    let mut grid = sim.take_overlay_grid_for_load().unwrap();
    let remove: Vec<_> = grid
        .iter_occupied()
        .filter(|(x, y, cell)| {
            registry
                .flags(cell.overlay_id.unwrap())
                .is_some_and(|flags| flags.tiberium)
                && !keep(*x, *y)
        })
        .map(|(x, y, _)| (x, y))
        .collect();
    for (x, y) in remove {
        grid.clear_overlay(x, y);
        grid.recalculate_runtime_cell(
            sim.resolved_terrain_mut_for_test().as_mut().unwrap(),
            registry,
            (x, y),
        );
    }
    sim.install_overlay_grid_for_load(grid);
}

#[test]
#[ignore = "slow; requires retail assets, an X11 display and GPU for full campaign loading"]
#[allow(
    deprecated,
    reason = "test creates an X11 loading surface without an app event loop"
)]
fn retail_all05_campaign_miner_obeys_shroud_through_normal_loading_and_frames() {
    use crate::map::source::test_support::TestDirectory;
    use crate::sim::scenario_session::ScenarioGameMode;
    use winit::platform::x11::EventLoopBuilderExtX11;
    let (root, mut assets) = crate::rules::retail_ini_fixture::retail_assets().unwrap();
    let source = assets.get_ref("ALL05UMD.MAP").unwrap().to_vec();
    assert_eq!(
        crate::util::sha256::sha256_hex(&source),
        "7ee8148ad4b0092b7970588d9b064ddd6b753fc7bfb352bcc043affeb72c6aed"
    );
    let derived = controlled_all05(&source, None);
    let directory = TestDirectory::new("campaign-miner-shroud");
    directory.write("ALL05UMD.MAP", &derived);
    if let Some(path) = std::env::var_os("VERA20K_CAMPAIGN_MINER_REPRO_DIR") {
        let path = std::path::PathBuf::from(path);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("ALL05UMD.MAP"), &derived).unwrap();
    }
    let events = winit::event_loop::EventLoop::builder()
        .with_any_thread(true)
        .build()
        .unwrap();
    let window = std::sync::Arc::new(
        events
            .create_window(
                winit::window::Window::default_attributes()
                    .with_visible(false)
                    .with_inner_size(winit::dpi::PhysicalSize::new(800, 600)),
            )
            .unwrap(),
    );
    let gpu = GpuContext::new(window).unwrap();
    let batch = BatchRenderer::new(&gpu);
    // Ask the production mission to select an actually reachable distant
    // stock ore cell; distance is fixture admission, not another scan port.
    let mut probe = load_campaign(&root, &mut assets, directory.path(), &gpu, &batch);
    let id = miner(&probe);
    keep_ore(&mut probe, |x, y| {
        x.abs_diff(52) >= 10 || y.abs_diff(93) >= 10
    });
    let owner = probe.simulation.session().current_house().unwrap();
    probe
        .simulation
        .fog_mut_for_test()
        .reveal_all_for_owner(owner);
    let target = dispatch(&mut probe, id).expect("ALL05 must offer reachable distant ore");
    eprintln!("retail campaign miner selected reachable control cell {target:?}");
    let computer_owner = probe
        .simulation
        .houses()
        .iter()
        .find(|(_, house)| !house.is_human() && !house.multiplay_passive())
        .map(|(&owner, _)| probe.simulation.interner().resolve(owner).to_owned())
        .expect("ALL05 has a computer House");
    for (mode, visible, computer, expected) in [
        (ScenarioGameMode::Campaign, false, false, None),
        (ScenarioGameMode::Campaign, true, false, Some(target)),
        (ScenarioGameMode::Skirmish, false, false, Some(target)),
        (ScenarioGameMode::Campaign, false, true, Some(target)),
    ] {
        // Supply another authored House through the normal map constructor;
        // do not overwrite the retained ownership flag in an entity fixture.
        directory.write(
            "ALL05UMD.MAP",
            &controlled_all05(&source, computer.then_some(computer_owner.as_str())),
        );
        let mut runtime = load_campaign(&root, &mut assets, directory.path(), &gpu, &batch);
        let id = miner(&runtime);
        keep_ore(&mut runtime, |x, y| (x, y) == target);
        let sim = &mut runtime.simulation;
        sim.session_mut_for_test().set_game_mode(mode);
        let owner = sim.session().current_house().unwrap();
        let miner_owner = sim.entities().get(id).unwrap().owner();
        assert_eq!(miner_owner == owner, !computer);
        let refinery = sim.interner().get("GAREFN").unwrap();
        assert!(
            sim.entities()
                .values()
                .any(|entity| entity.owner() == miner_owner && entity.type_ref() == refinery)
        );
        if computer {
            assert!(!sim.houses()[&miner_owner].is_human());
        }
        sim.fog_mut_for_test().reset_explored_for_owner(owner);
        if visible {
            sim.fog_mut_for_test().reveal_all_for_owner(owner);
        }
        let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(
            sim.resolved_terrain().unwrap(),
        );
        assert_eq!(
            crate::sim::vision::cell_is_shrouded(sim.fog(), &cells, owner, target),
            !visible
        );
        assert_eq!(
            dispatch(&mut runtime, id),
            expected,
            "{mode:?}, visible={visible}, computer={computer}"
        );
    }
}
