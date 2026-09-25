use super::*;

/// The random-map seed file the `.SED` launch branch recognises. Written into
/// the RA2 directory so `map_load` finds it where the original puts it.
const RANDMAP_SED_FILE: &str = "RandMap.Sed";
/// Description the setup dialog stamps onto a randomized configuration; it also
/// becomes the sentinel row's displayed name.
pub(super) const RANDOM_MAP_DESCRIPTION_KEY: &str = "TXT_RANDOM_MAP_DESCRIPTION";
pub(super) const RANDOM_MAP_DESCRIPTION_FALLBACK: &str = "Random Map";
/// The players slider is the last of the setup dialog's six option rows, and the
/// dialog gives it a range of 2..8 with a step of one.
const SETUP_PLAYERS_ROW: usize = 5;
const SETUP_PLAYERS_MIN: i32 = 2;
const SETUP_PLAYERS_MAX: i32 = 8;
const SETUP_PLAYERS_STEP: i32 = 1;
/// Matches the rules default the map-load path falls back to; the preview only
/// needs it because terrain resolution takes it, not because cliffs affect the
/// image.
const RANDOM_MAP_PREVIEW_CLIFF_BACK_IMPASSABILITY: u8 = 2;
/// Where the generated preview is written. The chooser's sentinel row reads this
/// back, so writing it is what makes the random-map thumbnail appear there.
const RANDMAP_PREVIEW_FILE: &str = "RandMap.img";

/// Native RMG map-storage identity used by its guarded Resize predicate.
///
/// `RandomMapGenerator @ 0x00599D48` compares the normalized current theater,
/// player count, width, and height against its cached MapSeed clone at
/// `0x00599B8E..0x00599BD3`. Other options can regenerate content in the same
/// storage and therefore do not participate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RandomMapStorageKey {
    theater: i32,
    num_players: i32,
    width: i32,
    height: i32,
}

impl RandomMapStorageKey {
    fn from_options(options: &crate::map::rmg::RmgOptions) -> Self {
        let mut normalized = options.clone();
        normalized.normalize();
        Self {
            theater: normalized.theater,
            num_players: normalized.num_players,
            width: normalized.width,
            height: normalized.height,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RandomMapStorageDecision {
    key: RandomMapStorageKey,
    reinitialize: bool,
}

impl RandomMapStorageDecision {
    /// Apply only the guarded native Resize side effect. Preview redraws never
    /// receive this decision and cannot reconstruct the process dummy.
    fn apply_resize_to(&self, shared_cell_dummy: &crate::map::resolved_terrain::SharedCellDummy) {
        if self.reinitialize {
            // The JZ at 0x00599D48..0x00599D95 skips this call when the cached
            // tuple matches. On a miss, MapClass::Resize @ 0x00565C10 invokes
            // CellClass::Constructor @ 0x0047BBF0 on the fixed dummy in place.
            shared_cell_dummy.reconstruct_for_map_resize();
        }
    }
}

/// Resolve one RMG preview without touching process-global MapClass state.
fn build_random_map_preview_grid(
    map_file: &crate::map::map_file::MapFile,
    theater: Option<&crate::map::theater::TheaterData>,
    asset_manager: Option<&crate::assets::asset_manager::AssetManager>,
    terrain_rules: Option<&crate::rules::terrain_rules::TerrainRules>,
    frontend_main_rng: &mut crate::sim::rng::SimRng,
    selector_cache: &mut crate::map::tile_variant_selector::TileVariantSelectorCache,
) -> crate::map::resolved_terrain::ResolvedTerrainGrid {
    let mut raw_draw = || frontend_main_rng.next_u32();
    let mut selector = selector_cache.begin_load(&mut raw_draw);
    // RMG InitMap supplies explicit Clear cells. Its preview never borrows a
    // Scenario cursor; equal-bound Fill remains zero-cost.
    let mut scenario_fill_ranged = |low, high| {
        debug_assert_eq!((low, high), (0, 0));
        0
    };
    crate::map::resolved_terrain::ResolvedTerrainGrid::build_with_variant_selector(
        map_file,
        theater,
        asset_manager,
        terrain_rules,
        None,
        None,
        false,
        RANDOM_MAP_PREVIEW_CLIFF_BACK_IMPASSABILITY,
        &mut scenario_fill_ranged,
        &mut selector,
    )
}

/// A random-map generation handed to a worker thread.
///
/// The worker only *generates*; colouring a preview needs the terrain resolver
/// and therefore the asset manager, so the main thread rasterises everything the
/// worker hands it. What matters is that the expensive part is off the UI
/// thread — that is what lets frames render while it runs.
pub(crate) struct RandomMapGenerationJob {
    receiver: std::sync::mpsc::Receiver<RandomMapUpdate>,
    /// Kept back from the worker because rasterising needs it: the resolver
    /// reads theater data to decide each cell's final tile.
    theater: Box<crate::map::theater::TheaterData>,
    terrain_rules: Box<crate::rules::terrain_rules::TerrainRules>,
    /// Set when OK started this generation. Accept cannot run until the map
    /// exists, so it is deferred to whoever collects the result.
    accept_on_finish: bool,
}

/// Provenance-bearing copy of the eight Scenario start-staging cells written
/// by the accepted random-map setup run.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct AcceptedRmgStartStaging {
    starts: Vec<crate::map::waypoints::Waypoint>,
}

impl AcceptedRmgStartStaging {
    fn from_generated(generated: &crate::map::rmg::GeneratedMap) -> Self {
        let mut slots = [None; crate::skirmish_launch::SKIRMISH_PLAYER_SLOT_COUNT];
        for &(slot, rx, ry) in &generated.start_waypoints {
            if let Some(entry) = slots.get_mut(usize::from(slot)) {
                *entry = Some(crate::map::waypoints::Waypoint {
                    index: u32::from(slot),
                    rx,
                    ry,
                });
            }
        }
        Self {
            starts: slots.into_iter().flatten().collect(),
        }
    }

    pub(crate) fn into_waypoint_table(
        self,
    ) -> std::collections::HashMap<u32, crate::map::waypoints::Waypoint> {
        self.starts
            .into_iter()
            .map(|waypoint| (waypoint.index, waypoint))
            .collect()
    }

    /// Borrow-only fact projection used to validate the complete prefix before
    /// surrendering the unique accepted-staging token.
    pub(crate) fn waypoint_table_for_admission(
        &self,
    ) -> std::collections::HashMap<u32, crate::map::waypoints::Waypoint> {
        self.starts
            .iter()
            .copied()
            .map(|waypoint| (waypoint.index, waypoint))
            .collect()
    }
}

/// Accepted random-map artifacts split at the app/loading boundary. The large
/// generated map remains presentation-only; only `start_staging` may supply
/// gameplay start cells.
#[derive(Debug)]
pub(crate) struct AcceptedRandomMapLaunch {
    presentation_preview: crate::map::rmg::GeneratedMap,
    start_staging: AcceptedRmgStartStaging,
}

impl AcceptedRandomMapLaunch {
    pub(crate) fn into_parts(self) -> (crate::map::rmg::GeneratedMap, AcceptedRmgStartStaging) {
        (self.presentation_preview, self.start_staging)
    }
}

/// Accepted launch-artifact ownership across setup and loading composition.
///
/// The candidate belongs only to the open setup dialog. The accepted bundle
/// carries both presentation fallback and exact start staging for the matching
/// `.SED`; gameplay map data still regenerates through the seed-file reader
/// after the match reseed.
/// gamemd provenance: accepted caller 0x005E8590 persists `RandMap.Sed`, while
/// Scenario read 0x00684620 unconditionally reaches the generator again.
#[derive(Default)]
pub(crate) struct RandomMapGenerationRetention {
    candidate: Option<crate::map::rmg::GeneratedMap>,
    accepted_launch: Option<(String, AcceptedRandomMapLaunch)>,
    /// The native MapSeed+0x178 clone survives repeated Generate/OK work while
    /// the dialog is open. A four-field mismatch replaces its backing storage,
    /// and common dialog teardown destroys it before a later reopen.
    map_storage_key: Option<RandomMapStorageKey>,
}

impl RandomMapGenerationRetention {
    pub(super) fn map_storage_decision(
        &self,
        options: &crate::map::rmg::RmgOptions,
    ) -> RandomMapStorageDecision {
        let key = RandomMapStorageKey::from_options(options);
        RandomMapStorageDecision {
            key,
            reinitialize: self.map_storage_key != Some(key),
        }
    }

    pub(super) fn commit_map_storage_decision(&mut self, decision: RandomMapStorageDecision) {
        self.map_storage_key = Some(decision.key);
    }

    pub(super) fn destroy_map_storage(&mut self) {
        // RandomMapSetupDialog__Run @ 0x00595BC0 destroys DAT_00ABE150 and
        // nulls it at 0x00595CB2..0x00595CC2 whenever the modal returns.
        self.map_storage_key = None;
    }

    pub(super) fn begin_generation(&mut self) {
        self.candidate = None;
        self.accepted_launch = None;
    }

    pub(super) fn finish_generation(&mut self, generated: crate::map::rmg::GeneratedMap) {
        self.candidate = Some(generated);
    }

    fn cancel_setup(&mut self) {
        self.candidate = None;
        self.accepted_launch = None;
    }

    pub(super) fn accept_setup(&mut self, selected_map_file: &str) {
        self.accepted_launch = self.candidate.take().map(|generated| {
            let start_staging = AcceptedRmgStartStaging::from_generated(&generated);
            (
                selected_map_file.to_owned(),
                AcceptedRandomMapLaunch {
                    presentation_preview: generated,
                    start_staging,
                },
            )
        });
    }

    pub(super) fn select_map(&mut self, selected_map_file: &str) {
        if self
            .accepted_launch
            .as_ref()
            .is_some_and(|(accepted_file, _)| {
                !accepted_file.eq_ignore_ascii_case(selected_map_file)
            })
        {
            self.accepted_launch = None;
        }
    }

    pub(super) fn take_acceptance_for_loading(
        &mut self,
        selected_map_file: Option<&str>,
    ) -> Option<AcceptedRandomMapLaunch> {
        let (accepted_file, accepted) = self.accepted_launch.take()?;
        selected_map_file
            .is_some_and(|selected| accepted_file.eq_ignore_ascii_case(selected))
            .then_some(accepted)
    }

    /// Presentation-only compatibility accessor used by lifecycle tests.
    /// Production transfers the complete accepted value so staging cannot be
    /// dropped or reconstructed from the preview.
    #[cfg(test)]
    pub(super) fn take_preview_for_loading(
        &mut self,
        selected_map_file: Option<&str>,
    ) -> Option<crate::map::rmg::GeneratedMap> {
        self.take_acceptance_for_loading(selected_map_file)
            .map(|accepted| accepted.presentation_preview)
    }
}

/// What the generator worker sends back as it goes.
pub(super) enum RandomMapUpdate {
    /// Cross-thread receipt emitted immediately before the worker enters the
    /// real generator. Unlike a test-thread counter, this follows the run that
    /// the production receiver actually owns.
    Started,
    /// The map at one of the boundaries the original redraws its preview at.
    Progress(Box<crate::map::rmg::build::GenerationSnapshot>),
    /// The finished map.
    Finished(Box<crate::map::rmg::GeneratedMap>),
}

/// Plain generation inputs resolved on the app thread before the worker is
/// spawned. This is the production preparation boundary used by both live UI
/// generation and the retail lifecycle proof.
pub(super) struct PreparedRandomMapGeneration {
    pub(super) theater: crate::map::theater::TheaterData,
    pub(super) terrain_rules: crate::rules::terrain_rules::TerrainRules,
    pub(super) settings: crate::map::rmg::RmgSettings,
    pub(super) resolved_inputs: crate::map::rmg::build::ResolvedTheaterInputs,
    pub(super) blocks: crate::map::rmg::theater_blocks::TheaterTileBlocks,
    pub(super) tech_types: Vec<crate::map::rmg::phases::tech_buildings::TechType>,
}

pub(super) fn prepare_random_map_generation(
    asset_manager: &mut crate::assets::asset_manager::AssetManager,
    options: &crate::map::rmg::RmgOptions,
) -> Option<PreparedRandomMapGeneration> {
    let settings = crate::map::rmg::RmgSettings::load(asset_manager);
    let theater_name = crate::map::rmg::emit::theater_name(options.theater);
    let Some(theater) = crate::map::theater::load_theater(asset_manager, theater_name) else {
        log::warn!("random map: theater {theater_name} unavailable");
        return None;
    };
    let terrain_rules = asset_manager
        .get_ref("rulesmd.ini")
        .and_then(|bytes| crate::rules::ini_parser::IniFile::from_bytes(bytes).ok())
        .map(|ini| crate::rules::terrain_rules::TerrainRules::from_ini(&ini))
        .unwrap_or_default();
    let resolved_inputs = crate::map::rmg::build::ResolvedTheaterInputs::from_theater(
        &theater,
        &terrain_rules,
        crate::map::rmg::trig::global().cloned(),
    );
    let blocks =
        crate::map::rmg::theater_blocks::TheaterTileBlocks::build(&theater.lookup, |name| {
            asset_manager.get(name)
        });
    let tech_types = crate::app::loading::init_helpers::load_neutral_tech_types(asset_manager);
    Some(PreparedRandomMapGeneration {
        theater,
        terrain_rules,
        settings,
        resolved_inputs,
        blocks,
        tech_types,
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn spawn_random_map_generation_worker(
    options: crate::map::rmg::RmgOptions,
    settings: crate::map::rmg::RmgSettings,
    resolved_inputs: crate::map::rmg::build::ResolvedTheaterInputs,
    blocks: crate::map::rmg::theater_blocks::TheaterTileBlocks,
    tech_types: Vec<crate::map::rmg::phases::tech_buildings::TechType>,
    shared_cell_dummy: crate::map::resolved_terrain::SharedCellDummy,
    map_storage_decision: RandomMapStorageDecision,
) -> std::io::Result<std::sync::mpsc::Receiver<RandomMapUpdate>> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("random-map-generate".to_string())
        .spawn(move || {
            map_storage_decision.apply_resize_to(&shared_cell_dummy);
            // This receipt is ordered directly ahead of the call, on the same
            // worker. It therefore proves a production generation entry even
            // when the caller and worker use different test threads.
            let _ = sender.send(RandomMapUpdate::Started);
            let generated = crate::map::rmg::build::generate_map_observed(
                &options,
                &settings,
                &resolved_inputs,
                &blocks,
                &tech_types,
                // A closed receiver means the dialog went away; dropping
                // what we produce is the correct outcome, not an error.
                &mut |view| {
                    if !draws_preview(view.point()) {
                        return;
                    }
                    let _ = sender.send(RandomMapUpdate::Progress(Box::new(view.snapshot())));
                },
            );
            if generated.unfilled_start_slots > 0 {
                log::warn!(
                    "Random map is short of spawns: {} start slot(s) could \
                     not be filled; those players have no start position",
                    generated.unfilled_start_slots
                );
            }
            let _ = sender.send(RandomMapUpdate::Finished(Box::new(generated)));
        })?;
    Ok(receiver)
}

/// Apply the finished worker receipt to the same shell owners the live poll
/// mutates. The return value tells the caller whether deferred OK may now run.
pub(super) fn finish_random_map_generation_owners(
    runtime: &mut crate::app::frontend::skirmish_session::OfflineSkirmishRuntime,
    retention: &mut RandomMapGenerationRetention,
    modal: &mut crate::ui::skirmish_shell::RandomMapSetupModalState,
    generated: crate::map::rmg::GeneratedMap,
    preview: Option<crate::map::rmg::preview::PreviewImage>,
    accept_on_finish: bool,
) -> bool {
    runtime.replay_random_map_preview_construction(&generated.construction_trace);
    retention.finish_generation(generated);
    modal.finish_generate(preview);
    accept_on_finish
        && matches!(
            modal.accept(),
            crate::ui::skirmish_shell::AcceptOutcome::Commit(_)
        )
}

/// The non-presentation state transition at each Generate/implicit-OK entry.
/// It is shared by the live App entry and the retail lifecycle harness so a
/// repeated run always invalidates the same candidate and accepted launch.
pub(super) fn begin_random_map_generation_owners(
    runtime: &mut crate::app::frontend::skirmish_session::OfflineSkirmishRuntime,
    retention: &mut RandomMapGenerationRetention,
    options: &crate::map::rmg::RmgOptions,
) {
    runtime.remember_random_map_options(options);
    retention.begin_generation();
}

/// Serialize the completed dialog preview at the native common-teardown
/// boundary. Passing no directory models startup without a configured retail
/// install; teardown itself still proceeds.
fn write_random_map_preview_to_dir(
    ra2_dir: Option<&std::path::Path>,
    preview: &crate::map::rmg::preview::PreviewImage,
) {
    let Some(ra2_dir) = ra2_dir else {
        return;
    };
    let (Ok(width), Ok(height)) = (u16::try_from(preview.width), u16::try_from(preview.height))
    else {
        log::warn!(
            "random map: preview {}x{} does not fit a PCX header",
            preview.width,
            preview.height
        );
        return;
    };
    let rgb: Vec<u8> = preview
        .rgba
        .chunks_exact(4)
        .flat_map(|px| {
            crate::render::native_surface_format::ACTIVE_RETAIL_RGB565_PRESENTATION
                .storage_roundtrip_rgb8([px[0], px[1], px[2]])
        })
        .collect();
    match crate::assets::pcx_file::encode_direct_rgb(width, height, &rgb) {
        Ok(encoded) => {
            let path = ra2_dir.join(RANDMAP_PREVIEW_FILE);
            if let Err(err) = std::fs::write(&path, encoded) {
                log::warn!("random map: could not write {}: {err}", path.display());
            }
        }
        Err(err) => log::warn!("random map: could not encode the preview: {err}"),
    }
}

/// The setup runner's production-owned common teardown: publish the last
/// completed preview, destroy the modal, and release the cached MapClass
/// storage. Both Cancel and accepted OK use this exact owner.
///
/// gamemd-derived: `RandomMapSetupDialog__Run @ 0x00595BC0` writes
/// `RandMap.img` at 0x00595C17 before destroying `DAT_00ABE150` at
/// 0x00595CB2..0x00595CC2; only afterward does accepted caller 0x005E8590
/// write `RandMap.Sed` and install the chooser sentinel.
fn dismiss_random_map_setup_owners(
    modal: &mut Option<crate::ui::skirmish_shell::RandomMapSetupModalState>,
    retention: &mut RandomMapGenerationRetention,
    ra2_dir: Option<&std::path::Path>,
) {
    if let Some(preview) = modal
        .as_ref()
        .and_then(|modal| modal.generated_preview.as_ref())
    {
        write_random_map_preview_to_dir(ra2_dir, preview);
    }
    *modal = None;
    retention.destroy_map_storage();
}

pub(super) fn cancel_random_map_setup_owners(
    runtime: &mut crate::app::frontend::skirmish_session::OfflineSkirmishRuntime,
    modal: &mut Option<crate::ui::skirmish_shell::RandomMapSetupModalState>,
    retention: &mut RandomMapGenerationRetention,
    ra2_dir: Option<&std::path::Path>,
) -> Option<crate::ui::skirmish_shell::ChooseMapSelection> {
    let modal_ref = modal.as_ref()?;
    runtime.remember_random_map_options(&modal_ref.options);
    let previous_selection = modal_ref.cancel();
    dismiss_random_map_setup_owners(modal, retention, ra2_dir);
    retention.cancel_setup();
    previous_selection
}

/// Select the candidate, run common teardown, and return the normalized seed
/// options that the accepted caller writes only afterward.
pub(super) fn accept_random_map_setup_owners(
    runtime: &mut crate::app::frontend::skirmish_session::OfflineSkirmishRuntime,
    modal: &mut Option<crate::ui::skirmish_shell::RandomMapSetupModalState>,
    retention: &mut RandomMapGenerationRetention,
    ra2_dir: Option<&std::path::Path>,
) -> Option<crate::map::rmg::RmgOptions> {
    let crate::ui::skirmish_shell::AcceptOutcome::Commit(options) = modal.as_ref()?.accept() else {
        return None;
    };
    runtime.remember_random_map_options(&options);
    retention.accept_setup(RANDMAP_SED_FILE);
    dismiss_random_map_setup_owners(modal, retention, ra2_dir);
    Some(*options)
}

/// Whether the original redraws its preview at this generation boundary.
///
/// It draws eight times while generating, reporting 55, 60, 70, 80, 85, 90 and
/// 95 percent on the seven after the first. Two of those pairs have no
/// generation between them at all — only a progress-report helper runs — so the
/// 60 and 85 redraws reproduce the image already on screen and are dropped here:
/// eight calls, six distinct pictures.
///
/// The percentages are the anchor the boundaries below were chosen from; they
/// have no home in the port yet, because the dialog's progress bar is still
/// drawn empty.
fn draws_preview(point: crate::map::rmg::build::GenerationPoint) -> bool {
    use crate::map::rmg::Stage;
    use crate::map::rmg::build::GenerationPoint;
    matches!(
        point,
        // Clears the box before any terrain exists.
        GenerationPoint::Initial
            // 55 (and again at 60): the water is in.
            | GenerationPoint::After(Stage::WaterFinalize)
            // 70: regions, island passes and the green spread.
            | GenerationPoint::After(Stage::RecalcAfterTerrain)
            // 80 (and again at 85): starts, tech buildings and tiberium.
            | GenerationPoint::After(Stage::RecalcAfterTiberium)
            // 90: the hills.
            | GenerationPoint::After(Stage::Hills)
            // 95: LAT patches, trees and rocks.
            | GenerationPoint::After(Stage::Rocks)
    )
}

impl App {
    /// Kick off generation on a worker and return immediately.
    ///
    /// Everything that needs the asset manager is done here, up front; only
    /// plain data crosses to the worker.
    pub(super) fn start_random_map_generation(
        state: &mut AppState,
        options: &crate::map::rmg::RmgOptions,
        accept_on_finish: bool,
    ) -> bool {
        // The ordinary Generate path also stamps the localized working label
        // immediately before generator construction (0x00595BC0).
        let mut generation_options = options.clone();
        generation_options.description = Self::csf_label(state, RANDOM_MAP_DESCRIPTION_KEY, RANDOM_MAP_DESCRIPTION_FALLBACK).into();
        if let Some(modal) = state.frontend.skirmish_shell_state.random_map_setup_modal.as_mut() {
            modal.options.description = generation_options.description.clone();
        }
        let options = &generation_options;
        // A second Generate makes the previous dialog result stale immediately,
        // even when setup cannot progress far enough to spawn the worker.
        begin_random_map_generation_owners(
            &mut state.frontend.offline_skirmish_runtime,
            &mut state.frontend.random_map_retention,
            options,
        );
        let (manager, tile_cache) = state.process_assets.manager_mut_with_tile_cache();
        let Some(asset_manager) = manager else {
            return false;
        };
        let Some(PreparedRandomMapGeneration {
            theater,
            terrain_rules,
            settings,
            resolved_inputs,
            blocks,
            tech_types,
        }) = prepare_random_map_generation(asset_manager, options)
        else {
            return false;
        };
        // Stock RMG preview publishes its resolved theater registry before the
        // later ordinary map load, even if generation subsequently fails.
        tile_cache.complete_theater_registry_load(
            theater.rmg_tiles.clear_tile,
            theater.rmg_tiles.water_set,
        );
        let options = options.clone();
        let shared_cell_dummy = state.process_assets.shared_cell_dummy.clone();
        let map_storage_decision = state
            .frontend
            .random_map_retention
            .map_storage_decision(&options);
        // Generation stays single-threaded and seed-driven; the thread changes
        // only where it runs, never the order it consumes its RNG in.
        match spawn_random_map_generation_worker(
            options,
            settings,
            resolved_inputs,
            blocks,
            tech_types,
            shared_cell_dummy,
            map_storage_decision,
        ) {
            Ok(receiver) => {
                state
                    .frontend
                    .random_map_retention
                    .commit_map_storage_decision(map_storage_decision);
                state.frontend.random_map_generation = Some(RandomMapGenerationJob {
                    receiver,
                    theater: Box::new(theater),
                    terrain_rules: Box::new(terrain_rules),
                    accept_on_finish,
                });
                true
            }
            Err(err) => {
                log::warn!("random map: could not spawn the generator thread: {err}");
                false
            }
        }
    }

    /// Collect whatever the generator has produced since the last frame.
    ///
    /// Called every frame while a job is in flight. Returns true when the dialog
    /// changed, so the caller knows to redraw.
    ///
    /// Only the newest of several progress snapshots is rasterised. Colouring is
    /// the expensive half, and an image the worker overtook before a frame was
    /// drawn was never on screen to be seen.
    pub(crate) fn poll_random_map_generation(state: &mut AppState) -> bool {
        if state.frontend.random_map_generation.is_some()
            && state
                .frontend
                .skirmish_shell_state
                .random_map_setup_modal
                .is_none()
        {
            // The dialog went away without the job going with it. Drop it here
            // rather than trusting every close path to remember: a job with no
            // dialog has nowhere to deliver, and letting it finish would write
            // a preview file for a map nobody asked for.
            state.frontend.random_map_generation = None;
            return false;
        }
        let Some(job) = state.frontend.random_map_generation.as_ref() else {
            return false;
        };
        let mut latest_progress = None;
        let mut finished = None;
        let mut died = false;
        loop {
            match job.receiver.try_recv() {
                Ok(RandomMapUpdate::Started) => {}
                Ok(RandomMapUpdate::Progress(snapshot)) => latest_progress = Some(snapshot),
                Ok(RandomMapUpdate::Finished(generated)) => {
                    finished = Some(generated);
                    break;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    died = true;
                    break;
                }
            }
        }

        if let Some(generated) = finished {
            let job = state
                .frontend
                .random_map_generation
                .take()
                .expect("checked present above");
            let preview = Self::rasterise_generated_map(state, &job, &generated);
            let generated = *generated;
            let accept = {
                let frontend = &mut state.frontend;
                match frontend
                    .skirmish_shell_state
                    .random_map_setup_modal
                    .as_mut()
                {
                    Some(modal) => finish_random_map_generation_owners(
                        &mut frontend.offline_skirmish_runtime,
                        &mut frontend.random_map_retention,
                        modal,
                        generated,
                        preview,
                        job.accept_on_finish,
                    ),
                    None => false,
                }
            };
            if accept {
                Self::accept_random_map_setup(state);
            }
            return true;
        }

        if died {
            // The worker ended without a result. Clear the job so the dialog
            // does not sit disabled forever waiting on it.
            log::warn!("random map: the generator thread ended without a result");
            state.frontend.random_map_generation = None;
            if let Some(modal) = state
                .frontend
                .skirmish_shell_state
                .random_map_setup_modal
                .as_mut()
            {
                modal.fail_generate();
            }
            return true;
        }

        let Some(snapshot) = latest_progress else {
            return false;
        };
        // Lifted out and put straight back: rasterising reads the job and the
        // rest of the app state at once, and the job lives inside that state.
        let job = state
            .frontend
            .random_map_generation
            .take()
            .expect("checked present above");
        let preview =
            Self::rasterise_map(state, &job, &snapshot.map_file, &snapshot.start_waypoints);
        state.frontend.random_map_generation = Some(job);
        if let Some(modal) = state
            .frontend
            .skirmish_shell_state
            .random_map_setup_modal
            .as_mut()
        {
            if let Some(preview) = preview {
                modal.show_progress_preview(preview);
            }
        }
        true
    }

    /// Cancel the setup dialog, abandoning any generation and every retained
    /// result associated with this random-map selection.
    ///
    /// Dropping the job drops the receiver, so a worker still going finds a
    /// closed channel on its next send and its remaining output goes nowhere.
    /// That matters beyond tidiness: a late finish would otherwise overwrite
    /// `RandMap.img`, changing the chooser's thumbnail to a map the player
    /// walked away from.
    fn cancel_random_map_setup(state: &mut AppState) {
        let ra2_dir = state
            .platform
            .game_config
            .as_ref()
            .map(|config| config.paths.ra2_dir.clone());
        state.frontend.random_map_generation = None;
        let frontend = &mut state.frontend;
        let _ = cancel_random_map_setup_owners(
            &mut frontend.offline_skirmish_runtime,
            &mut frontend.skirmish_shell_state.random_map_setup_modal,
            &mut frontend.random_map_retention,
            ra2_dir.as_deref(),
        );
    }

    /// Commit the dialog's options and close it. Shared by the immediate accept
    /// and the one deferred behind a generation.
    fn accept_random_map_setup(state: &mut AppState) {
        let ra2_dir = state
            .platform
            .game_config
            .as_ref()
            .map(|config| config.paths.ra2_dir.clone());
        let options = {
            let frontend = &mut state.frontend;
            accept_random_map_setup_owners(
                &mut frontend.offline_skirmish_runtime,
                &mut frontend.skirmish_shell_state.random_map_setup_modal,
                &mut frontend.random_map_retention,
                ra2_dir.as_deref(),
            )
        };
        let Some(options) = options else {
            return;
        };
        state.frontend.random_map_generation = None;
        match Self::commit_random_map_setup(state, &options) {
            Ok(()) => {
                // Teardown retained both accepted start staging and its
                // presentation preview; map construction remains owned by the
                // `.SED` reader.
            }
            Err(err) => {
                // The native dialog has already torn down at this point. Do
                // not leave an accepted launch attached when the seed/options
                // commit failed and no random-map selection was installed.
                state.frontend.random_map_retention.cancel_setup();
                log::error!("random map: could not write {RANDMAP_SED_FILE}: {err}");
            }
        }
    }

    /// Rasterise the finished map into the dialog-owned preview surface.
    /// Persistence belongs to common teardown, not generation completion.
    fn rasterise_generated_map(
        state: &mut AppState,
        job: &RandomMapGenerationJob,
        generated: &crate::map::rmg::GeneratedMap,
    ) -> Option<crate::map::rmg::preview::PreviewImage> {
        Self::rasterise_map(state, job, &generated.map_file, &generated.start_waypoints)
    }

    /// Colour and rasterise a map. Main thread only: the resolver reads theater
    /// data and the ore/gem colours come out of overlay SHPs.
    ///
    /// Mid-generation snapshots go through here too, so an in-progress preview
    /// is coloured by exactly the path that colours the finished one.
    fn rasterise_map(
        state: &mut AppState,
        job: &RandomMapGenerationJob,
        map_file: &crate::map::map_file::MapFile,
        start_waypoints: &[(u8, u16, u16)],
    ) -> Option<crate::map::rmg::preview::PreviewImage> {
        // LAT defaults off for runtime maps, so resolve the same way the load
        // path will; a different setting here would colour cells the player
        // never sees.
        let resolved_terrain = {
            let frontend_main_rng = &mut state.frontend.frontend_main_rng;
            let (manager, selector_cache) = state.process_assets.manager_mut_with_tile_cache();
            let asset_manager = manager.map(|m| &*m);
            build_random_map_preview_grid(
                map_file,
                Some(&job.theater),
                asset_manager,
                Some(&job.terrain_rules),
                frontend_main_rng,
                selector_cache,
            )
        };
        // Ore and gem cells take their colour from the overlay's own SHP: the
        // growth stage indexes the frame list and the frame header carries the
        // radar triple. The artwork is never sampled for it, so there is no
        // substitute for loading the file.
        let overlay_registry = state.overlay_registry();
        let assets = state.process_assets.manager();
        let theater_ext = job.theater.extension;
        let overlay_radar = |overlay_id: u8, stage: u8| -> Option<[u8; 3]> {
            let registry = overlay_registry?;
            // The tiberium flag is the gate: walls, roads and bridges are
            // overlays too, and they keep the terrain colour underneath.
            if !registry.flags(overlay_id)?.tiberium {
                return None;
            }
            // The stage's colour out of the overlay SHP wins; the type's
            // RadarColor= stands in when that comes back essentially black,
            // which is also what happens when the art is missing entirely.
            let from_art = (|| {
                let name = registry.name(overlay_id)?;
                let bytes =
                    crate::render::overlay_assets::overlay_shp_candidates(name, theater_ext)
                        .iter()
                        .find_map(|candidate| assets?.get_ref(candidate))?;
                let shp = crate::assets::shp_file::ShpFile::from_bytes(bytes).ok()?;
                Some(shp.frames.get(stage as usize)?.radar_color)
            })()
            .filter(|rgb| *rgb != [0, 0, 0]);
            from_art.or_else(|| registry.flags(overlay_id)?.radar_color)
        };
        let cells = crate::map::rmg::preview::preview_cells_from_map(
            map_file,
            &resolved_terrain,
            &overlay_radar,
        );
        let waypoints = crate::map::rmg::preview::marker_waypoints(start_waypoints);
        crate::map::rmg::preview::render_preview(&cells, &waypoints)
    }

    /// Persist accepted random-map setup, refresh the sentinel record, and
    /// select it so launch generates from it.
    ///
    /// A failed write is fatal to the commit: `map_load` treats a missing seed
    /// file as "use defaults", so committing anyway would silently start a
    /// different map than the one the player configured.
    fn commit_random_map_setup(
        state: &mut AppState,
        options: &crate::map::rmg::RmgOptions,
    ) -> anyhow::Result<()> {
        let ra2_dir = state
            .platform
            .game_config
            .as_ref()
            .map(|config| config.paths.ra2_dir.clone())
            .ok_or_else(|| anyhow::anyhow!("no game config; cannot locate the RA2 directory"))?;
        std::fs::write(ra2_dir.join(RANDMAP_SED_FILE), options.to_sed_bytes())?;

        let description_text = options.description.display_text();
        let display = if options.description.is_empty() {
            RANDOM_MAP_DESCRIPTION_FALLBACK
        } else {
            &description_text
        };
        // Reuse the modal helper: it upserts the single sentinel, honours the
        // mode's random-map admission, and refreshes the filtered record list.
        let chooser_layout = crate::ui::skirmish_shell::compute_choose_map_modal_layout(
            state.render_width(),
            state.render_height(),
        );
        let Some(modal) = state
            .frontend
            .skirmish_shell_state
            .choose_map_modal
            .as_mut()
        else {
            return Ok(());
        };
        // F11: the catalog's mutation guard re-projects the shell map entries
        // on drop, so the loadable-map projection can never drift from the
        // records — the old hand-patch by name-position is gone.
        let index = {
            let mut records = state.frontend.scenario_catalog.records_mut();
            modal.create_random_map(
                &mut records,
                &state.frontend.skirmish_modes,
                display,
                options.num_players,
                &chooser_layout,
            )
        };
        let mode_id = modal.selected_mode_id;
        let _ = modal;
        if let Some(index) = index {
            let selection = crate::ui::skirmish_shell::ChooseMapSelection {
                mode_id,
                record_index: Some(index),
            };
            // Success runs Use Map (`0x005E6B2F`): its eject box can keep the
            // chooser (`0x005E6B47`).
            if Self::prompt_choose_map_eject(state, selection) {
                return Ok(());
            }
            let _ = Self::commit_choose_map_selection(state, selection);
            Self::close_choose_map_modal(state);
        }
        Ok(())
    }

    fn skirmish_random_map_setup_layout(
        state: &AppState,
    ) -> crate::ui::skirmish_shell::RandomMapSetupLayout {
        crate::ui::skirmish_shell::compute_random_map_setup_layout(
            state.render_width(),
            state.render_height(),
        )
    }

    pub(super) fn handle_random_map_setup_mouse_down(state: &mut AppState) -> bool {
        let layout = Self::skirmish_random_map_setup_layout(state);
        let x = state.match_state.input.cursor_x.round() as i32;
        let y = state.match_state.input.cursor_y.round() as i32;
        let Some(modal) = state
            .frontend
            .skirmish_shell_state
            .random_map_setup_modal
            .as_mut()
        else {
            return false;
        };
        // An open list covers the rows under it, so it gets first refusal on the
        // click. Clicking anywhere else closes it without acting on whatever is
        // underneath, the way a dismissed dropdown behaves.
        if let Some(combo) = modal.open_combo {
            let items = crate::ui::skirmish_shell::setup_combo_items(combo);
            let on_list = crate::ui::skirmish_shell::random_map_setup_dropdown_row_at(
                &layout,
                combo.row(),
                items.len(),
                x,
                y,
            )
            .is_some();
            let on_face = crate::ui::skirmish_shell::random_map_setup_control_at(&layout, x, y)
                == Some(combo.control());
            if !on_list && !on_face {
                modal.open_combo = None;
                return true;
            }
            if on_list {
                return true;
            }
        }
        if let Some(control) = crate::ui::skirmish_shell::random_map_setup_control_at(&layout, x, y)
        {
            // The players slider is not a button: it acts on press, not on
            // release, so it never arms a pressed control.
            if control == crate::ui::skirmish_shell::RandomMapSetupControl::Players0x3eb {
                if modal.is_enabled(control) {
                    Self::press_setup_players_trackbar(modal, &layout, x, y);
                }
                return true;
            }
            // A disabled control swallows the click without arming a press, so
            // releasing over it cannot fire.
            if modal.is_enabled(control) {
                modal.pressed_control = Some(control);
                Self::play_main_menu_button_sound(state);
            }
            return true;
        }
        layout.dialog.contains(x, y)
    }

    /// Press behaviour for the players slider, mirroring the shell's other
    /// trackbars: grabbing the thumb starts a tracking drag, while a press on
    /// the rail jumps the value once and tracks nothing.
    fn press_setup_players_trackbar(
        modal: &mut crate::ui::skirmish_shell::RandomMapSetupModalState,
        layout: &crate::ui::skirmish_shell::RandomMapSetupLayout,
        x: i32,
        y: i32,
    ) {
        let rect = layout.control_rects[SETUP_PLAYERS_ROW];
        if !crate::ui::skirmish_shell::trackbar_mouse_allowed_y(rect, y) {
            return;
        }
        let pixel_offset = crate::ui::skirmish_shell::trackbar_pixel_offset(
            modal.options.num_players,
            SETUP_PLAYERS_MIN,
            SETUP_PLAYERS_MAX,
            SETUP_PLAYERS_STEP,
            rect,
        );
        if crate::ui::skirmish_shell::trackbar_thumb_hit(rect, pixel_offset, x, y) {
            modal.dragging_players_thumb = true;
        } else if rect.contains(x, y) {
            modal.set_num_players(crate::ui::skirmish_shell::trackbar_mouse_value(
                rect,
                x,
                SETUP_PLAYERS_MIN,
                SETUP_PLAYERS_MAX,
                SETUP_PLAYERS_STEP,
            ));
        }
    }

    pub(super) fn handle_random_map_setup_mouse_move(state: &mut AppState) {
        let layout = Self::skirmish_random_map_setup_layout(state);
        let x = state.match_state.input.cursor_x.round() as i32;
        let Some(modal) = state
            .frontend
            .skirmish_shell_state
            .random_map_setup_modal
            .as_mut()
        else {
            return;
        };
        if !modal.dragging_players_thumb {
            return;
        }
        let rect = layout.control_rects[SETUP_PLAYERS_ROW];
        modal.set_num_players(crate::ui::skirmish_shell::trackbar_mouse_value(
            rect,
            x,
            SETUP_PLAYERS_MIN,
            SETUP_PLAYERS_MAX,
            SETUP_PLAYERS_STEP,
        ));
        state.platform.window.request_redraw();
    }

    pub(super) fn handle_random_map_setup_mouse_up(state: &mut AppState) -> bool {
        use crate::ui::skirmish_shell::RandomMapSetupControl as Control;

        let layout = Self::skirmish_random_map_setup_layout(state);
        let x = state.match_state.input.cursor_x.round() as i32;
        let y = state.match_state.input.cursor_y.round() as i32;
        // RMGMD.INI drives the randomizer's vegetation bounds; without it the
        // derived vegetation collapses to zero and randomized maps lose trees.
        let settings = state
            .process_assets
            .manager()
            .map(crate::map::rmg::RmgSettings::load)
            .unwrap_or_default();
        let description = state
            .process_assets
            .csf
            .as_ref()
            .map(|csf| csf.text(RANDOM_MAP_DESCRIPTION_KEY).into_owned())
            .unwrap_or_else(|| RANDOM_MAP_DESCRIPTION_FALLBACK.to_string());
        let Some(modal) = state
            .frontend
            .skirmish_shell_state
            .random_map_setup_modal
            .as_mut()
        else {
            return false;
        };
        if modal.dragging_players_thumb {
            modal.dragging_players_thumb = false;
            return true;
        }
        // Releasing over an open list commits that entry. The press was never
        // armed for list clicks, so this has to run before the pressed check.
        if let Some(combo) = modal.open_combo {
            let items = crate::ui::skirmish_shell::setup_combo_items(combo);
            if let Some(index) = crate::ui::skirmish_shell::random_map_setup_dropdown_row_at(
                &layout,
                combo.row(),
                items.len(),
                x,
                y,
            ) {
                modal.set_combo_value(combo, items[index].value);
                return true;
            }
        }
        let pressed = modal.pressed_control.take();
        let released = crate::ui::skirmish_shell::random_map_setup_control_at(&layout, x, y);
        if pressed.is_none() || pressed != released {
            return layout.dialog.contains(x, y) || pressed.is_some();
        }

        let mut close_setup = false;
        // Generating needs the whole app state, so it cannot run while the modal
        // is mutably borrowed; the actions below only record what to do.
        let mut generate_requested = false;
        let mut accept_requested = false;
        let mut open_browser: Option<SavedSeedMode> = None;
        match released.expect("checked equal to pressed control") {
            Control::Randomize0x621 => {
                modal.randomize_options(
                    &settings,
                    &mut state.frontend.frontend_main_rng,
                    &description,
                );
            }
            Control::Generate0x620 => {
                modal.reroll_derived_for_generate(&settings, &mut state.frontend.frontend_main_rng);
                modal.begin_generate();
                generate_requested = true;
            }
            Control::Ok0x6c5 => {
                // Accept generates first when nothing has been generated yet,
                // so the committed options always describe a map that exists.
                if matches!(
                    modal.accept(),
                    crate::ui::skirmish_shell::AcceptOutcome::NeedsGenerate
                ) {
                    modal.reroll_derived_for_generate(
                        &settings,
                        &mut state.frontend.frontend_main_rng,
                    );
                    modal.begin_generate();
                    generate_requested = true;
                }
                accept_requested = true;
            }
            Control::Cancel0x5c0 => {
                // Result 2 in the original: no seed file, no sentinel, no
                // selection change. The chooser underneath is left untouched.
                close_setup = true;
            }
            Control::Load0x6c2 => open_browser = Some(SavedSeedMode::Load),
            Control::Save0x6c3 => open_browser = Some(SavedSeedMode::Save),
            Control::Delete0x6c4 => open_browser = Some(SavedSeedMode::Delete),
            Control::MapType0x405
            | Control::Time0x3ea
            | Control::Theater0x407
            | Control::Size0x406
            | Control::Resources0x408 => {
                if let Some(combo) =
                    crate::ui::skirmish_shell::SetupCombo::from_control(released.expect("matched"))
                {
                    modal.toggle_combo(combo);
                }
            }
            // Dragging the players slider is a separate input mode; clicking the
            // track alone does not move it.
            Control::Players0x3eb => {}
        }

        if let Some(mode) = open_browser {
            Self::open_saved_seed_browser(state, mode);
            return true;
        }
        if generate_requested {
            let options = state
                .frontend
                .skirmish_shell_state
                .random_map_setup_modal
                .as_ref()
                .map(|modal| modal.options.clone());
            let started = options.is_some_and(|options| {
                Self::start_random_map_generation(state, &options, accept_requested)
            });
            if !started {
                // Nothing will arrive, so the dialog must not be left sitting
                // in its generating state with every control disabled.
                log::warn!("random map: could not start generation for the configured options");
                if let Some(modal) = state
                    .frontend
                    .skirmish_shell_state
                    .random_map_setup_modal
                    .as_mut()
                {
                    modal.fail_generate();
                }
            }
            // Accept, if it was asked for, is now the job's responsibility.
            return true;
        }
        if accept_requested {
            Self::accept_random_map_setup(state);
        }
        if close_setup {
            Self::cancel_random_map_setup(state);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gsi_04_12_preview_file_is_teardown_owned_and_precedes_accepted_commit() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let base = std::env::temp_dir().join(format!(
            "vera20k-rmg-teardown-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&base).expect("create teardown fixture directory");
        let img_path = base.join(RANDMAP_PREVIEW_FILE);
        let sed_path = base.join(RANDMAP_SED_FILE);
        let options = crate::map::rmg::RmgOptions {
            seed: 0x412,
            ..Default::default()
        };
        let preview = crate::map::rmg::preview::PreviewImage {
            width: 1,
            height: 1,
            rgba: vec![0x12, 0x34, 0x56, 0xFF],
        };
        let mut runtime =
            crate::app::frontend::skirmish_session::OfflineSkirmishRuntime::initialize(
                0x0412_959B,
                None,
                None,
                None,
                crate::app::frontend::skirmish_session::skirmish_global_defaults(
                    &crate::ui::skirmish_shell::SkirmishShellState::default(),
                ),
            );
        let mut retention = RandomMapGenerationRetention::default();
        retention.finish_generation(generated_preview(0x412, 10));
        let mut modal = Some(crate::ui::skirmish_shell::RandomMapSetupModalState::open(
            options, None, false,
        ));
        modal
            .as_mut()
            .expect("accepted modal")
            .finish_generate(Some(preview.clone()));

        // Worker completion only fills dialog memory. Until common teardown,
        // the chooser file remains absent. The production accepted owner then
        // publishes it before returning options to the `.SED` caller.
        assert!(!img_path.exists());
        let committed =
            accept_random_map_setup_owners(&mut runtime, &mut modal, &mut retention, Some(&base))
                .expect("generated modal accepts");
        assert!(img_path.exists(), "common teardown must publish .img first");
        assert!(!sed_path.exists(), ".SED cannot precede common teardown");
        std::fs::write(&sed_path, committed.to_sed_bytes()).expect("write accepted seed");
        assert!(sed_path.exists());

        // Cancel runs the same teardown writer but never reaches the accepted
        // caller, so it updates only .img.
        std::fs::remove_file(&sed_path).expect("remove accepted seed fixture");
        let mut cancel_retention = RandomMapGenerationRetention::default();
        cancel_retention.finish_generation(generated_preview(0x413, 10));
        let mut cancel_modal = Some(crate::ui::skirmish_shell::RandomMapSetupModalState::open(
            crate::map::rmg::RmgOptions {
                seed: 0x413,
                ..Default::default()
            },
            None,
            false,
        ));
        cancel_modal
            .as_mut()
            .expect("cancel modal")
            .finish_generate(Some(preview));
        let _ = cancel_random_map_setup_owners(
            &mut runtime,
            &mut cancel_modal,
            &mut cancel_retention,
            Some(&base),
        );
        assert!(img_path.exists());
        assert!(!sed_path.exists());

        std::fs::remove_file(&img_path).expect("remove preview fixture");
        std::fs::remove_dir(&base).expect("remove teardown fixture directory");
    }

    #[test]
    fn six_preview_boundaries_cover_the_originals_eight_redraws() {
        use crate::map::rmg::STAGE_ORDER;
        use crate::map::rmg::build::GenerationPoint;

        let drawn: Vec<GenerationPoint> = std::iter::once(GenerationPoint::Initial)
            .chain(
                STAGE_ORDER
                    .iter()
                    .map(|stage| GenerationPoint::After(*stage)),
            )
            .filter(|point| draws_preview(*point))
            .collect();
        // Eight redraws in the original, two of them repeats of the image
        // already on screen.
        assert_eq!(drawn.len(), 6, "{drawn:?}");
        assert_eq!(drawn[0], GenerationPoint::Initial);
        // The last one precedes the final recalc, so the finished map still
        // differs from it and the closing draw is not a repeat either.
        assert_eq!(
            drawn[5],
            GenerationPoint::After(crate::map::rmg::Stage::Rocks)
        );
    }

    fn apply_storage_decision_for_test(
        retention: &mut RandomMapGenerationRetention,
        options: &crate::map::rmg::RmgOptions,
        process_dummy: &crate::map::resolved_terrain::SharedCellDummy,
    ) -> bool {
        let decision = retention.map_storage_decision(options);
        decision.apply_resize_to(process_dummy);
        retention.commit_map_storage_decision(decision);
        decision.reinitialize
    }

    #[test]
    fn gsi_04_01_rmg_resize_predicate_matches_cached_native_tuple() {
        let mut retention = RandomMapGenerationRetention::default();
        let process_dummy = crate::map::resolved_terrain::SharedCellDummy::fresh();
        process_dummy.set_level_slope(-7, 11);
        process_dummy.stamp_coord(7, 9);
        let options = crate::map::rmg::RmgOptions::default();

        retention.begin_generation();
        assert!(apply_storage_decision_for_test(
            &mut retention,
            &options,
            &process_dummy
        ));
        assert_eq!(
            process_dummy.snapshot(),
            crate::map::resolved_terrain::SharedCellDummySnapshot {
                coord: (0, 0),
                level: 0,
                slope_type: 0,
                bridge_flags_0x1180: 0,
            }
        );

        process_dummy.set_level_slope(-3, 5);
        process_dummy.stamp_coord(12, -4);
        let expected = process_dummy.snapshot();
        let mut same_storage = options.clone();
        same_storage.seed = 0x401;
        same_storage.map_type = 4;
        same_storage.resources = 3;
        retention.begin_generation();
        assert!(!apply_storage_decision_for_test(
            &mut retention,
            &same_storage,
            &process_dummy
        ));
        assert_eq!(
            process_dummy.snapshot(),
            expected,
            "repeated Generate in one open dialog reuses the cached RMG map storage"
        );

        let changes: [(&str, fn(&mut crate::map::rmg::RmgOptions)); 4] = [
            ("theater", |changed| changed.theater = 1),
            ("num_players", |changed| changed.num_players = 3),
            ("width", |changed| changed.width = 1),
            ("height", |changed| changed.height = 1),
        ];
        for (field, change) in changes {
            let mut isolated_retention = RandomMapGenerationRetention::default();
            let isolated_dummy = crate::map::resolved_terrain::SharedCellDummy::fresh();
            assert!(apply_storage_decision_for_test(
                &mut isolated_retention,
                &options,
                &isolated_dummy
            ));
            isolated_dummy.set_level_slope(-6, 9);
            isolated_dummy.stamp_coord(8, -10);
            let mut changed = options.clone();
            change(&mut changed);
            assert_ne!(
                RandomMapStorageKey::from_options(&changed),
                RandomMapStorageKey::from_options(&options),
                "{field} fixture must remain distinct after native normalization"
            );
            assert!(apply_storage_decision_for_test(
                &mut isolated_retention,
                &changed,
                &isolated_dummy
            ));
            assert_eq!(
                isolated_dummy.snapshot(),
                crate::map::resolved_terrain::SharedCellDummySnapshot {
                    coord: (0, 0),
                    level: 0,
                    slope_type: 0,
                    bridge_flags_0x1180: 0,
                }
            );
        }
    }

    #[test]
    fn gsi_04_01_rmg_dialog_teardown_destroys_cached_storage_on_ok_and_cancel() {
        let options = crate::map::rmg::RmgOptions::default();

        for accepted in [true, false] {
            let close_path = if accepted { "OK" } else { "Cancel" };
            let mut retention = RandomMapGenerationRetention::default();
            let process_dummy = crate::map::resolved_terrain::SharedCellDummy::fresh();

            retention.begin_generation();
            assert!(apply_storage_decision_for_test(
                &mut retention,
                &options,
                &process_dummy
            ));
            retention.finish_generation(generated_preview(0x401, 10));
            if accepted {
                retention.accept_setup(RANDMAP_SED_FILE);
            } else {
                retention.cancel_setup();
            }
            // The common owner destroys native map storage after preview
            // disposition has already been selected.
            retention.destroy_map_storage();

            process_dummy.set_level_slope(-4, 7);
            process_dummy.stamp_coord(13, -9);
            retention.begin_generation();
            assert!(
                apply_storage_decision_for_test(&mut retention, &options, &process_dummy),
                "the first Generate after {close_path} must allocate fresh RMG map storage"
            );
            assert_eq!(
                process_dummy.snapshot(),
                crate::map::resolved_terrain::SharedCellDummySnapshot {
                    coord: (0, 0),
                    level: 0,
                    slope_type: 0,
                    bridge_flags_0x1180: 0,
                },
                "the first Generate after {close_path} must run MapClass::Resize"
            );
        }
    }

    #[test]
    fn gsi_04_01_rmg_preview_resolution_does_not_reconstruct_dummy() {
        let process_dummy = crate::map::resolved_terrain::SharedCellDummy::fresh();
        process_dummy.set_level_slope(-3, 5);
        process_dummy.stamp_coord(12, -4);
        let expected = process_dummy.snapshot();
        let map = generated_preview(11, 10).map_file;
        let mut frontend_main_rng = crate::sim::rng::SimRng::new(0x0401_599D);
        let mut selector_cache =
            crate::map::tile_variant_selector::TileVariantSelectorCache::default();
        for _ in 0..6 {
            let preview_grid = build_random_map_preview_grid(
                &map,
                None,
                None,
                None,
                &mut frontend_main_rng,
                &mut selector_cache,
            );
            assert!(
                !preview_grid
                    .shared_cell_dummy()
                    .same_identity(&process_dummy)
            );
            assert_eq!(
                process_dummy.snapshot(),
                expected,
                "preview rasterization must not run another MapClass Resize"
            );
        }
    }

    fn generated_preview(seed: i32, start_x: u16) -> crate::map::rmg::GeneratedMap {
        let mut options = crate::map::rmg::RmgOptions::default();
        options.seed = seed;
        crate::map::rmg::GeneratedMap {
            map_file: crate::map::rmg::emit::empty_map_file(&options, 32, 32),
            mapgen_continuation: crate::map::rmg::RmgRng::new(seed as u16).into_continuation(),
            construction_trace: crate::map::rmg::RmgConstructionTrace::default(),
            start_waypoints: vec![(0, start_x, 20)],
            stages_run: Vec::new(),
            unfilled_start_slots: 0,
        }
    }

    #[test]
    fn gsi_04_12_random_map_preview_invalidates_and_transfers_exactly_once() {
        let mut regenerated = RandomMapGenerationRetention::default();
        regenerated.finish_generation(generated_preview(11, 10));
        regenerated.accept_setup("RandMap.Sed");
        regenerated.begin_generation();
        assert!(
            regenerated
                .take_preview_for_loading(Some("RandMap.Sed"))
                .is_none(),
            "starting a genuine regeneration invalidates accepted map A"
        );

        let mut reopened_then_cancelled_without_generate = RandomMapGenerationRetention::default();
        reopened_then_cancelled_without_generate.finish_generation(generated_preview(12, 11));
        reopened_then_cancelled_without_generate.accept_setup("RandMap.Sed");
        reopened_then_cancelled_without_generate.cancel_setup();
        assert!(
            reopened_then_cancelled_without_generate
                .take_preview_for_loading(Some("RandMap.Sed"))
                .is_none(),
            "a genuine setup Cancel invalidates accepted map A"
        );

        let mut reopened_then_cancelled = RandomMapGenerationRetention::default();
        reopened_then_cancelled.finish_generation(generated_preview(13, 12));
        reopened_then_cancelled.accept_setup("RandMap.Sed");
        reopened_then_cancelled.begin_generation();
        reopened_then_cancelled.finish_generation(generated_preview(14, 13));
        reopened_then_cancelled.cancel_setup();
        assert!(
            reopened_then_cancelled
                .take_preview_for_loading(Some("RandMap.Sed"))
                .is_none(),
            "reopen, regenerate, then Cancel cannot resurrect accepted map A"
        );

        let mut cancelled = RandomMapGenerationRetention::default();
        cancelled.finish_generation(generated_preview(22, 20));
        cancelled.cancel_setup();
        cancelled.accept_setup("RandMap.Sed");
        assert!(
            cancelled
                .take_preview_for_loading(Some("RandMap.Sed"))
                .is_none()
        );

        let mut selected_elsewhere = RandomMapGenerationRetention::default();
        selected_elsewhere.finish_generation(generated_preview(33, 30));
        selected_elsewhere.accept_setup("RandMap.Sed");
        selected_elsewhere.select_map("mp01t4.map");
        assert!(
            selected_elsewhere
                .take_preview_for_loading(Some("RandMap.Sed"))
                .is_none()
        );

        let mut alternate_seed = RandomMapGenerationRetention::default();
        alternate_seed.finish_generation(generated_preview(34, 35));
        alternate_seed.accept_setup("RandMap.Sed");
        // This is the retention authority used by apply_selected_shell_map_file:
        // another seed selection must invalidate, not merely refuse this call.
        alternate_seed.select_map("Other.Sed");
        assert!(
            alternate_seed
                .take_preview_for_loading(Some("RandMap.Sed"))
                .is_none()
        );
        assert!(
            alternate_seed
                .take_preview_for_loading(Some("Other.Sed"))
                .is_none()
        );

        let mut mismatched_launch = RandomMapGenerationRetention::default();
        mismatched_launch.finish_generation(generated_preview(35, 36));
        mismatched_launch.accept_setup("RandMap.Sed");
        assert!(
            mismatched_launch
                .take_preview_for_loading(Some("Other.Sed"))
                .is_none()
        );
        assert!(
            mismatched_launch
                .take_preview_for_loading(Some("RandMap.Sed"))
                .is_none(),
            "a refused nonmatching launch invalidates the prior accepted map"
        );

        let mut accepted_after_successful_close = RandomMapGenerationRetention::default();
        accepted_after_successful_close.finish_generation(generated_preview(44, 40));
        accepted_after_successful_close.accept_setup("RandMap.Sed");
        // Common dialog teardown destroys only the cached RMG map storage; it
        // must not discard the accepted launch bundle awaiting loading.
        accepted_after_successful_close.destroy_map_storage();
        accepted_after_successful_close.select_map("RANDMAP.SED");
        let transferred = accepted_after_successful_close
            .take_preview_for_loading(Some("randmap.sed"))
            .expect("successful accept close preserves the accepted preview");
        assert_eq!(transferred.start_waypoints, vec![(0, 40, 20)]);
        assert!(
            accepted_after_successful_close
                .take_preview_for_loading(Some("RandMap.Sed"))
                .is_none(),
            "successful accept preview still transfers exactly once"
        );
    }
}
