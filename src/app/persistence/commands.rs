//! User save/load requests; input and dialogs only select the operation.

use crate::app::audio_runtime::AppAudioRuntime;
use crate::app::diagnostics::state::DiagnosticsState;
use crate::app::match_runtime::sim_host::SimWrite;
use crate::app::match_runtime::state::MatchState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::{AppState, PlatformState};
use crate::ui::game_screen::GameScreen;

use std::path::{Path, PathBuf};

/// A save request can fail before disk I/O without changing the running match.
#[derive(Debug, thiserror::Error)]
pub(crate) enum SaveCommandError {
    #[error("Enter a name for the saved game.")]
    EmptyDescription,
    #[error("There is no active game to save.")]
    MissingSimulation,
    #[error("The active game has no authoritative source-map digest.")]
    MissingMapHash,
    #[error("The active game has no saved resource context: {0}")]
    MissingResources(&'static str),
    #[error("Could not capture saved terrain: {0}")]
    Terrain(#[from] crate::map::resolved_terrain::SavedTerrainError),
    #[error("Could not capture the local view: {0}")]
    LocalView(#[from] crate::sim::snapshot::saved_local_view::SavedLocalViewError),
    #[error("Could not encode the saved game: {0}")]
    Encode(#[source] crate::util::save_codec::Error),
    #[error("Could not save the game: {0}")]
    Write(#[from] crate::app::persistence::SaveWriteError),
}

#[cfg(feature = "dev-overlays")]
pub(crate) fn quicksave(
    platform: &PlatformState,
    renderer: &RendererState,
    screen: &GameScreen,
    match_state: &mut MatchState,
    persistence: &super::PersistenceState,
    audio: &AppAudioRuntime,
    diag: &DiagnosticsState,
    process_assets: &ProcessAssets,
) {
    log_save_result(
        "Quicksave",
        save(
            platform,
            renderer,
            screen,
            match_state,
            persistence,
            audio,
            diag,
            process_assets,
            None,
            SaveDestination::Player(None),
        ),
    );
}

/// UI save/create/explicit overwrite request. A new timestamp-name collision
/// is an error, never implicit permission to overwrite an existing save.
/// This is the VERA snapshot format; it does not claim native SAV compatibility.
pub(crate) fn save_game(
    platform: &PlatformState,
    renderer: &RendererState,
    screen: &GameScreen,
    match_state: &mut MatchState,
    persistence: &super::PersistenceState,
    audio: &AppAudioRuntime,
    diag: &DiagnosticsState,
    process_assets: &ProcessAssets,
    description: &str,
    overwrite: Option<&Path>,
) -> Result<PathBuf, SaveCommandError> {
    save(
        platform,
        renderer,
        screen,
        match_state,
        persistence,
        audio,
        diag,
        process_assets,
        Some(description),
        SaveDestination::Player(overwrite),
    )
}

/// The opt-in autosave (`persistence::autosave`): the match under
/// `description` in the one autosave slot.
pub(crate) fn autosave_game(
    platform: &PlatformState,
    renderer: &RendererState,
    screen: &GameScreen,
    match_state: &mut MatchState,
    persistence: &super::PersistenceState,
    audio: &AppAudioRuntime,
    diag: &DiagnosticsState,
    process_assets: &ProcessAssets,
    description: &str,
) -> Result<PathBuf, SaveCommandError> {
    save(
        platform,
        renderer,
        screen,
        match_state,
        persistence,
        audio,
        diag,
        process_assets,
        Some(description),
        SaveDestination::Autosave,
    )
}

enum SaveDestination<'a> {
    Player(Option<&'a Path>),
    /// `autosave::FILE_NAME`, replacing the previous autosave.
    Autosave,
    InitialCampaign(&'a str),
}

/// Gather immutable app projections without snapshotting the live Simulation.
/// Only the process Rules borrow escapes; this lets the frame mutate its world
/// before the exact MainTick55DBCD save boundary.
pub(crate) fn prepare_initial_campaign_save<'a>(
    platform: &PlatformState,
    renderer: &RendererState,
    screen: &GameScreen,
    match_state: &MatchState,
    persistence: &super::PersistenceState,
    audio: &AppAudioRuntime,
    diag: &DiagnosticsState,
    rules_owner: Option<&'a crate::rules::process_owner::NativeRulesProcessOwner>,
) -> Option<Result<PreparedSave<'a>, SaveCommandError>> {
    let campaign = match_state
        .sim_runtime
        .runtime()
        .is_some_and(|runtime| !runtime.simulation.session().game_mode_nonzero());
    persistence
        .initial_campaign_save_pending(campaign)
        .then(|| {
            prepare_save(
                platform,
                renderer,
                screen,
                match_state,
                persistence,
                audio,
                diag,
                Some(&format!("[{}]", match_state.campaign_names.save_title())),
                rules_owner,
            )
        })
}

/// Called only by the frame's post-input, post-Ground, pre-Logic boundary.
/// The same writer serves manual and automatic saves; the process latch is
/// consumed here before either a preparation error or an IO attempt.
pub(crate) fn initial_campaign_save(
    prepared: Option<Result<PreparedSave<'_>, SaveCommandError>>,
    sim: &crate::sim::world::Simulation,
    persistence: &mut super::PersistenceState,
    clock: &mut crate::app::match_runtime::frame_pacer::ScenarioElapsedClock,
) {
    let Some(prepared) = prepared else {
        return;
    };
    if !persistence.take_initial_campaign_save(!sim.session().game_mode_nonzero()) {
        return;
    }
    let filename = initial_campaign_filename(sim.session().map_name());
    let result = prepared.and_then(|prepared| {
        prepared.write(
            sim,
            &persistence.repository,
            clock,
            SaveDestination::InitialCampaign(&filename),
        )
    });
    match result {
        Ok(path) => log::info!("Initial campaign save: {}", path.display()),
        Err(error) => log::warn!("Initial campaign save failed: {error}"),
    }
}

/// Native strrchr scans the whole stored Scenario filename, then appends .sav.
/// The repository separately admits the path it can own; do not sanitize or
/// turn this into Path::file_stem, which treats a dot in a directory differently.
fn initial_campaign_filename(scenario: &str) -> String {
    let stem = scenario.rsplit_once('.').map_or(scenario, |(stem, _)| stem);
    format!("{stem}.sav")
}

fn save(
    platform: &PlatformState,
    renderer: &RendererState,
    screen: &GameScreen,
    match_state: &mut MatchState,
    persistence: &super::PersistenceState,
    audio: &AppAudioRuntime,
    diag: &DiagnosticsState,
    process_assets: &ProcessAssets,
    name: Option<&str>,
    destination: SaveDestination<'_>,
) -> Result<PathBuf, SaveCommandError> {
    let prepared = prepare_save(
        platform,
        renderer,
        screen,
        match_state,
        persistence,
        audio,
        diag,
        name,
        process_assets.native_rules(),
    )?;
    prepared.write(
        &match_state
            .sim_runtime
            .runtime()
            .ok_or(SaveCommandError::MissingSimulation)?
            .simulation,
        &persistence.repository,
        &mut match_state.scenario_elapsed_clock,
        destination,
    )
}

/// App-owned projections frozen for one synchronous save. Simulation remains
/// borrowed only at write(), after the frame's input and Ground owners ran.
pub(crate) struct PreparedSave<'a> {
    map_hash: u64,
    rules_hash: u64,
    description: String,
    sanitized: Option<String>,
    timestamp: u64,
    sinking_waterlines: Vec<(u64, i16)>,
    lighting: crate::app::presentation::lighting::MatchLighting,
    sidebar_order: Option<crate::sim::snapshot::SavedSidebarOrder>,
    tactical_view: crate::sim::snapshot::SavedTacticalView,
    local_view: crate::sim::snapshot::saved_local_view::SavedLocalView,
    resources: crate::sim::snapshot::scenario_resources::SavedScenarioResourcesRef<'a>,
}

fn prepare_save<'a>(
    platform: &PlatformState,
    renderer: &RendererState,
    screen: &GameScreen,
    match_state: &MatchState,
    persistence: &super::PersistenceState,
    audio: &AppAudioRuntime,
    diag: &DiagnosticsState,
    name: Option<&str>,
    rules_owner: Option<&'a crate::rules::process_owner::NativeRulesProcessOwner>,
) -> Result<PreparedSave<'a>, SaveCommandError> {
    let sanitized = name
        .map(|raw| {
            super::SaveRepository::filename_part(raw).ok_or(SaveCommandError::EmptyDescription)
        })
        .transpose()?;
    let runtime = match_state
        .sim_runtime
        .runtime()
        .ok_or(SaveCommandError::MissingSimulation)?;
    let map_hash = match_state
        .loaded_map_hash
        .ok_or(SaveCommandError::MissingMapHash)?;
    let sim = &runtime.simulation;
    let now = crate::util::time::SystemTime::now()
        .duration_since(crate::util::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut sinking_waterlines = match_state
        .match_presentation
        .sinking_waterlines
        .borrow()
        .saved();
    sinking_waterlines.retain(|(id, _)| sim.entities().contains(*id));
    let presentation = &match_state.match_presentation;
    let active_tab = saved_active_tab(presentation.active_sidebar_tab);
    let sidebar_order = presentation
        .sidebar_projection
        .model
        .saved_order()
        .map(|order| order.with_active_tab(active_tab));
    let tactical_view = crate::app::input::camera::saved_tactical_view(
        platform,
        renderer,
        screen,
        match_state,
        persistence,
    );
    let rules_owner = rules_owner.ok_or(SaveCommandError::MissingResources("process rules"))?;
    let processed =
        rules_owner
            .saved_scenario_rules()
            .ok_or(SaveCommandError::MissingResources(
                "processed scenario rules",
            ))?;
    let terrain = runtime
        .resources
        .terrain_template
        .as_ref()
        .ok_or(SaveCommandError::MissingResources("terrain template"))?
        .capture_saved_template()?;
    let source = match_state
        .loaded_map_source
        .as_ref()
        .ok_or(SaveCommandError::MissingResources("map source"))?;
    let presentation = &match_state.match_presentation;
    let mut local_view = match_state.input.capture_saved_local_view(
        sim,
        &runtime.resources.rules,
        &presentation.sidebar_gadget_state,
    )?;
    local_view.set_power_bar(presentation.power_bar_anim.saved());
    local_view.set_radar_housing(
        presentation
            .radar_anim
            .as_ref()
            .map(crate::render::radar::anim::RadarAnimState::saved_housing),
    );
    local_view.set_credits_counter(match_state.local_player_owner().and_then(|owner| {
        presentation
            .sidebar_projection
            .model
            .saved_credits_counter(owner)
    }));
    local_view.set_radar_events(presentation.minimap.as_ref().map_or_else(
        Default::default,
        crate::render::radar::minimap::MinimapRenderer::saved_radar_events,
    ));
    log::debug!(
        "Save: captured Power at {}ms: {:?}",
        crate::app::diagnostics::state::radar_presentation_ms(diag, match_state),
        presentation.power_bar_anim,
    );
    let map = crate::sim::snapshot::scenario_resources::SavedMapMetadata::capture(
        map_hash,
        source,
        &match_state.map_basic,
        &match_state.map_ranking,
        &match_state.campaign_names,
        &presentation.terrain_objects,
        &presentation.house_roster,
        &presentation.house_color_map,
        presentation.lighting.saved_config(),
        0, // Patched from the authoritative clock at the actual save boundary.
        match_state.session.local_player_name(),
        audio.theme.saved_slots(),
        presentation.radar_movies.scenario_hold(),
    )
    // Save_Game's VocClass::SaveStaticSounds (`0x00751620`): the map's
    // retained coordinate sounds, so a load can restart its ambient loops.
    // RESIDUAL: the audio owner keeps the list, so a session with no sound
    // device saves none and a later load with sound stays quiet there.
    .with_static_sounds(
        audio
            .sfx_player
            .as_ref()
            .map(crate::audio::sfx::SfxPlayer::saved_static_sounds)
            .unwrap_or_default(),
    );
    let resources = crate::sim::snapshot::scenario_resources::SavedScenarioResourcesRef::capture(
        &runtime.resources,
        processed,
        rules_owner.fixed_art().content_hash(),
        terrain,
        map,
    );
    Ok(PreparedSave {
        map_hash,
        rules_hash: runtime.resources.rules.simulation_config_hash(),
        description: name.unwrap_or(sim.session().map_name()).to_owned(),
        sanitized,
        timestamp: now,
        sinking_waterlines,
        lighting: presentation.lighting.clone(),
        sidebar_order,
        tactical_view,
        local_view,
        resources,
    })
}

/// Mouse Save 5BE6D0 writes the open tab (ActiveTab, +0x539C) beside the
/// strips; every save path carries it ([`SavedSidebarOrder::active_tab`]).
///
/// [`SavedSidebarOrder::active_tab`]: crate::sim::snapshot::SavedSidebarOrder::active_tab
pub(crate) fn saved_active_tab(tab: crate::ui::sidebar::SidebarTab) -> u8 {
    tab.tab_index() as u8
}

impl PreparedSave<'_> {
    /// Refresh the process-owned Strip state after input/automatic PLACE and
    /// before MainTick55DBCD writes the save. The prepared resource borrows
    /// and the simulation snapshot keep their existing owners.
    pub(crate) fn refresh_sidebar_order(
        &mut self,
        order: Option<crate::sim::snapshot::SavedSidebarOrder>,
    ) {
        self.sidebar_order = order;
    }

    fn write(
        mut self,
        sim: &crate::sim::world::Simulation,
        repository: &super::SaveRepository,
        clock: &mut crate::app::match_runtime::frame_pacer::ScenarioElapsedClock,
        destination: SaveDestination<'_>,
    ) -> Result<PathBuf, SaveCommandError> {
        sim.zone_grid()
            .ok_or(SaveCommandError::MissingResources("retained navigation"))?;
        self.resources.capture_live_cell_land(
            sim.resolved_terrain()
                .ok_or(SaveCommandError::MissingResources("live terrain"))?,
        )?;
        let clock_save = clock.begin_save(crate::app::match_runtime::frame_pacer::wall_clock_ms());
        self.resources
            .set_elapsed_buckets(clock_save.accumulated_buckets());
        let (cell_lighting, anim_intensities) = self.lighting.saved_at(sim);
        self.local_view.set_lighting(cell_lighting);
        let bytes = crate::sim::snapshot::GameSnapshot::save_validated_with_presentation(
            sim,
            self.map_hash,
            self.rules_hash,
            &self.description,
            self.timestamp,
            &self.sinking_waterlines,
            &anim_intensities,
            self.sidebar_order.as_ref(),
            Some(&self.tactical_view),
            Some(&self.local_view),
            Some(&self.resources),
        );
        let result = bytes.map_err(SaveCommandError::Encode).and_then(|bytes| {
            match destination {
                SaveDestination::Player(overwrite) => repository.write_player_slot(
                    overwrite,
                    self.sanitized.as_deref(),
                    sim.session().tick(),
                    self.timestamp,
                    &bytes,
                ),
                SaveDestination::InitialCampaign(filename) => {
                    repository.write_initial_campaign(filename, &bytes)
                }
                SaveDestination::Autosave => repository.write_autosave(&bytes),
            }
            .map_err(Into::into)
        });
        // Keep synchronous save dwell out of the source clock, including failure.
        clock.finish_save(
            clock_save,
            crate::app::match_runtime::frame_pacer::wall_clock_ms(),
        );
        result
    }
}

#[cfg(feature = "dev-overlays")]
fn log_save_result(label: &str, result: Result<PathBuf, SaveCommandError>) {
    match result {
        Ok(path) => log::info!("{label}: saved game to {}", path.display()),
        Err(SaveCommandError::Write(error)) => match error.stage() {
            crate::app::persistence::SaveWriteStage::CreateDirectory => {
                log::error!("{label}: failed to create saves dir: {error}")
            }
            crate::app::persistence::SaveWriteStage::WriteFile => {
                log::error!("{label}: write failed: {error}")
            }
        },
        Err(error) => log::warn!("{label}: {error}"),
    }
}

#[cfg(feature = "dev-overlays")]
pub(crate) fn quickload(
    platform: &crate::app::PlatformState,
    persistence: &mut crate::app::persistence::PersistenceState,
) {
    let path = match persistence.repository.quickload_path_by_modified_time() {
        Some(p) => p,
        None => {
            log::warn!(
                "Quickload: no save files found in {}/",
                persistence.repository.directory().display()
            );
            return;
        }
    };
    request_saved_game_load(platform, persistence, path);
}

pub(crate) struct PendingSavedGameLoad {
    path: PathBuf,
    presented: bool,
}

impl PendingSavedGameLoad {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            presented: false,
        }
    }

    fn advance(&mut self, present: impl FnOnce() -> bool) -> Option<&Path> {
        if !self.presented {
            self.presented = present();
            return None;
        }
        Some(self.path.as_path())
    }
}

/// LoadOptions559D60 shows F0 before LoadGame67E440. Returning after that
/// frame gives the browser a paint opportunity before synchronous restoration.
pub(crate) fn request_saved_game_load(
    platform: &crate::app::PlatformState,
    persistence: &mut crate::app::persistence::PersistenceState,
    path: PathBuf,
) {
    if persistence.pending_load.is_none() {
        persistence.pending_load = Some(PendingSavedGameLoad::new(path));
        platform.window.request_redraw();
    }
}

/// Called before ordinary frame service; the retained browser owns failure UI.
pub(crate) fn pump_saved_game_load(state: &mut AppState) -> Option<bool> {
    let mut pending = state.persistence.pending_load.take()?;
    let Some(path) = pending.advance(|| {
        match crate::app::loading::pump::present_saved_game_loading(
            &mut state.platform,
            &mut state.renderer,
            &mut state.frontend,
            &mut state.match_state,
            &state.process_assets,
            &state.persistence,
        ) {
            Ok(presented) => presented,
            Err(error) => {
                // Like missing native progress artwork, presentation failure
                // must not make an otherwise loadable save inaccessible.
                log::warn!("Saved-game loading dialog presentation failed: {error:#}");
                true
            }
        }
    }) else {
        let presented = pending.presented;
        state.persistence.pending_load = Some(pending);
        state.platform.window.request_redraw();
        return Some(presented);
    };
    let from_menu = state.frontend.screen == crate::ui::game_screen::GameScreen::MainMenu;
    let from_browser = state
        .match_state
        .match_presentation
        .saved_game_browser
        .is_some();
    let result = restore_save_file(state, path);
    if from_menu {
        crate::app::App::finish_load_saved_game_from_menu(
            &mut state.frontend,
            &mut state.match_state.match_presentation,
            &state.process_assets,
            path,
            result,
        );
    } else if from_browser {
        crate::app::App::finish_saved_game_load(
            &mut state.platform,
            &state.renderer,
            &mut state.frontend,
            &mut state.match_state,
            &state.process_assets,
            &mut state.audio,
            &mut state.persistence,
            path,
            result,
        );
    } else if let Err(error) = result {
        #[cfg(feature = "dev-overlays")]
        log_prepared_load_error(path, &error);
        #[cfg(not(feature = "dev-overlays"))]
        log::warn!("Load: {}: {error}", path.display());
    }
    state.platform.window.request_redraw();
    Some(false)
}

/// Both browser routes prepare the saved world's own resources, then publish
/// through the one restore owner. Failed preparation keeps the current world
/// and selected browser rows available for retry.
pub(crate) fn try_load_save_file(
    state: &mut AppState,
    path: &Path,
) -> Result<(), crate::app::persistence::PreparedLoadError> {
    if let Err(error) = crate::app::loading::pump::present_saved_game_loading(
        &mut state.platform,
        &mut state.renderer,
        &mut state.frontend,
        &mut state.match_state,
        &state.process_assets,
        &state.persistence,
    ) {
        log::warn!("Saved-game loading dialog presentation failed: {error:#}");
    }
    restore_save_file(state, path)
}

fn restore_save_file(
    state: &mut AppState,
    path: &Path,
) -> Result<(), crate::app::persistence::PreparedLoadError> {
    // A network game offers no load; one peer swapping its world mid-match
    // would leave the others' lockstep.
    if state.match_state.live_network.is_some() {
        return Err(crate::app::persistence::PreparedLoadError::NetworkMatch);
    }
    // UI559DBC->69BB40->5BF450 precedes the lower load, including failure;
    // a failed load leaves the radar to wind its clip down itself.
    let wall_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
    crate::app::radar_movie::retire_for_load(
        &mut state.match_state.match_presentation,
        &mut state.audio,
        wall_ms,
    );
    crate::app::chrono_screen::retire(
        &state.frontend.screen,
        &mut state.match_state,
        &mut state.audio,
        wall_ms,
    );
    use crate::app::persistence::{PreparedLoadError, PreparedSavedGame, SavedGamePreparationView};
    let from_menu = state.frontend.screen == crate::ui::game_screen::GameScreen::MainMenu;
    let Some(mut assets) = state.process_assets.lease_for_loading() else {
        return Err(PreparedLoadError::Resources(
            "retail assets are unavailable".into(),
        ));
    };
    let prepared = (|| {
        state
            .process_assets
            .initialize_sources_if_needed(&assets)
            .map_err(PreparedLoadError::Resources)?;
        let rules_owner = state
            .process_assets
            .native_rules()
            .ok_or(PreparedLoadError::MissingRules)?;
        let load_clock = crate::app::match_runtime::frame_pacer::wall_clock_ms;
        PreparedSavedGame::from_repository(
            SavedGamePreparationView {
                repository: &state.persistence.repository,
                current_simulation: state
                    .match_state
                    .sim_runtime
                    .runtime()
                    .map(|runtime| &runtime.simulation),
                frontend_main: &state.frontend.frontend_main_rng,
                rules_owner,
                tile_cache: &state.process_assets.tile_variant_selector_cache,
                lighting: &state.match_state.match_presentation.lighting,
                load_clock: &load_clock,
            },
            &mut assets,
            path,
            &state.renderer.gpu,
            &state.renderer.batch_renderer,
            state
                .match_state
                .match_presentation
                .in_game_options
                .detail_level,
        )
    })();
    // Every recoverable outcome returns this exact process manager. Its save
    // preparation guard has already restored outgoing lookup order on error.
    state.process_assets.return_from_loading(assets);
    let prepared = prepared?;
    crate::app::match_runtime::restore::commit_prepared_load(state, path, prepared, from_menu);
    if from_menu {
        let runtime = state
            .match_state
            .sim_runtime
            .write_sim(SimWrite::Lifecycle)
            .expect("saved world committed");
        resume_menu_saved_game_rng(
            &mut runtime.simulation,
            &mut crate::app::match_bootstrap::OrdinaryMatchSeedClock,
        );
    }
    Ok(())
}

/// Main52E619 initializes the stock Scenario/Main streams after loading
/// from the menu. The custom snapshot path retains its saved continuation.
pub(super) fn resume_menu_saved_game_rng(
    simulation: &mut crate::sim::world::Simulation,
    clock: &mut impl crate::app::match_bootstrap::MatchSeedClock,
) {
    if simulation.retains_snapshot_continuation() {
        return;
    }
    let seed = crate::app::match_bootstrap::read_match_seed(clock);
    simulation.initialize_random_number_system(seed.value);
}

#[cfg(feature = "dev-overlays")]
fn log_prepared_load_error(
    path: &std::path::Path,
    error: &crate::app::persistence::PreparedLoadError,
) {
    use crate::app::persistence::PreparedLoadError;

    match error {
        PreparedLoadError::ReadFile(source) => {
            log::warn!("Load: could not read {}: {source}", path.display())
        }
        PreparedLoadError::MissingRules | PreparedLoadError::NetworkMatch => {
            log::warn!("Load: {error}")
        }
        #[cfg(test)]
        PreparedLoadError::MissingCurrentSimulation
        | PreparedLoadError::MissingMapHash
        | PreparedLoadError::MissingTerrainTemplate => log::error!("Load: {error}"),
        PreparedLoadError::Snapshot(source) => log::error!("Load: {source}"),
        PreparedLoadError::Restore(source) => {
            log::error!("Load: restoration validation failed: {source}")
        }
        PreparedLoadError::SidebarOrder(source) | PreparedLoadError::TacticalView(source) => {
            log::error!("Load: restoration validation failed: {source}")
        }
        PreparedLoadError::FactoryState(source) => {
            log::error!("Load: restoration validation failed: {source}")
        }
        PreparedLoadError::Resources(_)
        | PreparedLoadError::Rules(_)
        | PreparedLoadError::Terrain(_)
        | PreparedLoadError::RadarEvents(_)
        | PreparedLoadError::LocalView(_) => {
            log::error!("Load: {error}")
        }
    }
}

#[cfg(all(test, tests_app))]
mod save_name_tests {
    use crate::app::persistence::SaveRepository;
    use crate::sim::snapshot::GameSnapshot;
    use crate::sim::world::Simulation;

    #[test]
    fn empty_description_has_no_filename() {
        assert_eq!(SaveRepository::filename_part(""), None);
        assert_eq!(SaveRepository::filename_part("   "), None);
        assert_eq!(SaveRepository::filename_part("\t\n"), None);
    }

    #[test]
    fn strips_path_separators() {
        assert_eq!(
            SaveRepository::filename_part("../foo").as_deref(),
            Some(".._foo")
        );
        assert_eq!(
            SaveRepository::filename_part("a/b\\c").as_deref(),
            Some("a_b_c")
        );
    }

    #[test]
    fn strips_windows_reserved_chars() {
        assert_eq!(
            SaveRepository::filename_part("a:b*c?d\"e<f>g|h").as_deref(),
            Some("a_b_c_d_e_f_g_h")
        );
    }

    #[test]
    fn gsi_17_02_unicode_filename_cap_preserves_exact_envelope_description() {
        let raw_description = "保存".repeat(40);
        let filename_part = SaveRepository::filename_part(&raw_description).unwrap();
        assert_eq!(filename_part.chars().count(), 64);

        let mut sim = Simulation::new();
        sim.session_mut_for_test()
            .set_map_name_for_test("OFFICIAL.MAP".to_string());
        let bytes = GameSnapshot::save_validated(&sim, 1, 2, &raw_description, 3);
        let header = GameSnapshot::read_header(&bytes).expect("current VERA header");
        assert_eq!(header.description, raw_description);
    }
}

#[cfg(all(test, tests_app))]
mod pending_load_tests {
    use super::PendingSavedGameLoad;
    use std::path::PathBuf;

    #[test]
    fn restoration_waits_for_a_presented_loading_frame() {
        for path in ["menu-save.bin", "in-game-save.sav"] {
            let mut pending = PendingSavedGameLoad::new(PathBuf::from(path));
            // A lost/minimized surface must retry the loading frame first.
            assert!(pending.advance(|| false).is_none());
            // Successful presentation still returns to the event loop.
            assert!(pending.advance(|| true).is_none());
            assert_eq!(
                pending.advance(|| panic!("loading frame was already presented")),
                Some(std::path::Path::new(path))
            );
        }
    }
}

#[cfg(all(test, tests_app))]
mod initial_save_tests {
    use super::initial_campaign_filename;

    #[test]
    fn initial_campaign_filenames_match_original_main_tick() {
        let native = crate::test_fixture::json("tools/input_oracle/campaign_mission_titles.json");
        for row in native["initial_save_rows"].as_array().unwrap() {
            if let Some(expected) = row["filename"].as_str() {
                assert_eq!(
                    initial_campaign_filename(row["logical_filename"].as_str().unwrap()),
                    expected,
                    "{}",
                    row["name"]
                );
            }
        }
    }
}
