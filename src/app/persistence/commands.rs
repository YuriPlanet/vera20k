//! User save/load requests; input and dialogs only select the operation.

use crate::app::AppState;

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
    #[error("Could not save the game: {0}")]
    Write(#[from] crate::app::persistence::SaveWriteError),
}

#[cfg(feature = "dev-overlays")]
pub(crate) fn quicksave(state: &mut AppState) {
    log_save_result("Quicksave", save(state, None, None));
}

/// Save As retains the exact user description in the envelope; only the
/// filename is sanitized. Quick saves use the map name as their description.
#[cfg(feature = "dev-ui")]
pub(crate) fn save_with_name(state: &mut AppState, raw_name: &str) {
    log_save_result("Save As", save_game(state, raw_name, None));
}

/// UI save/create/explicit overwrite request. A new timestamp-name collision
/// is an error, never implicit permission to overwrite an existing save.
/// This is the VERA snapshot format; it does not claim native SAV compatibility.
pub(crate) fn save_game(
    state: &mut AppState,
    description: &str,
    overwrite: Option<&Path>,
) -> Result<PathBuf, SaveCommandError> {
    save(state, Some(description), overwrite)
}

fn save(
    state: &mut AppState,
    name: Option<&str>,
    overwrite: Option<&Path>,
) -> Result<PathBuf, SaveCommandError> {
    let sanitized = name.map(sanitize_save_name);
    if sanitized.as_deref() == Some("") {
        return Err(SaveCommandError::EmptyDescription);
    }
    let runtime = state
        .match_state
        .sim_runtime
        .as_ref()
        .ok_or(SaveCommandError::MissingSimulation)?;
    let map_hash = state
        .match_state
        .loaded_map_hash
        .ok_or(SaveCommandError::MissingMapHash)?;
    let sim = &runtime.simulation;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut sinking_waterlines = state
        .match_state
        .match_presentation
        .sinking_waterlines
        .borrow()
        .saved();
    sinking_waterlines.retain(|(id, _)| sim.entities().contains(*id));
    let sidebar_order = state
        .match_state
        .match_presentation
        .sidebar_projection
        .saved_order();
    let tactical_view = crate::app::input::camera::saved_tactical_view(state);
    let rules_owner = state
        .process_assets
        .native_rules()
        .ok_or(SaveCommandError::MissingResources("process rules"))?;
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
    let source = state
        .match_state
        .loaded_map_source
        .as_ref()
        .ok_or(SaveCommandError::MissingResources("map source"))?;
    let presentation = &state.match_state.match_presentation;
    let mut local_view = state.match_state.input.capture_saved_local_view(
        sim,
        &runtime.resources.rules,
        &presentation.sidebar_gadget_state,
    )?;
    local_view.set_power_bar(presentation.power_bar_anim.saved());
    log::debug!(
        "Save: captured Power at {}ms: {:?}",
        state.radar_presentation_ms(),
        presentation.power_bar_anim,
    );
    let clock_now = crate::app::match_runtime::frame_pacer::wall_clock_ms();
    let clock_save = state
        .match_state
        .scenario_elapsed_clock
        .begin_save(clock_now);
    let map = crate::sim::snapshot::scenario_resources::SavedMapMetadata::capture(
        map_hash,
        source,
        &state.match_state.map_basic,
        &presentation.terrain_objects,
        &presentation.house_roster,
        &presentation.house_color_map,
        presentation.lighting.saved_config(),
        clock_save.accumulated_buckets(),
        state.match_state.session.local_player_name(),
        state.audio.theme.saved_slots(),
    );
    let resources = crate::sim::snapshot::scenario_resources::SavedScenarioResourcesRef::capture(
        &runtime.resources,
        processed,
        rules_owner.fixed_art().content_hash(),
        terrain,
        map,
    );
    let bytes = crate::sim::snapshot::GameSnapshot::save_validated_with_presentation(
        sim,
        map_hash,
        runtime.resources.rules.simulation_config_hash(),
        name.unwrap_or(&sim.session.map_name),
        now,
        &sinking_waterlines,
        sidebar_order.as_ref(),
        Some(&tactical_view),
        Some(&local_view),
        Some(&resources),
    );
    let tick = sim.session.tick;
    let result = if let Some(path) = overwrite {
        state.persistence.overwrite_save(path, &bytes, tick)
    } else {
        let filename = match sanitized {
            Some(name) => format!("save_{name}_tick{tick}_{now}.bin"),
            None => format!("save_tick{tick}_{now}.bin"),
        };
        state.persistence.write_save(&filename, &bytes, tick)
    };
    // Keep I/O dwell out of the source clock. VERA also resumes on a failed
    // write, preserving its live-match failure policy.
    let clock_now = crate::app::match_runtime::frame_pacer::wall_clock_ms();
    state
        .match_state
        .scenario_elapsed_clock
        .finish_save(clock_save, clock_now);
    result.map_err(Into::into)
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

/// Sanitize a user-typed save name for use in a filename.
///
/// Replaces Windows-reserved characters (`/ \ : * ? " < > |`) with `_`,
/// trims surrounding whitespace, then caps at 64 chars. Returns an empty
/// string for empty/whitespace-only input.
fn sanitize_save_name(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(trimmed.len());
    for ch in trimmed.chars().take(64) {
        match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => out.push('_'),
            c if c.is_control() => out.push('_'),
            c => out.push(c),
        }
    }
    out
}

#[cfg(feature = "dev-overlays")]
pub(crate) fn quickload(state: &mut AppState) {
    let path = match state
        .persistence
        .repository
        .quickload_path_by_modified_time()
    {
        Some(p) => p,
        None => {
            log::warn!(
                "Quickload: no save files found in {}/",
                state.persistence.repository.directory().display()
            );
            return;
        }
    };
    load_save_file(state, &path);
}

/// Load a save file by path. Used by both quickload and the save/load panel.
#[cfg(feature = "dev-overlays")]
pub(crate) fn load_save_file(state: &mut AppState, path: &std::path::Path) {
    if let Err(error) = try_load_save_file(state, path) {
        log_prepared_load_error(path, &error);
    }
}

/// Both browser routes prepare the saved world's own resources, then publish
/// through the one restore owner. Failed preparation keeps the current world
/// and selected browser rows available for retry.
pub(crate) fn try_load_save_file(
    state: &mut AppState,
    path: &Path,
) -> Result<(), crate::app::persistence::PreparedLoadError> {
    // UI559DBC->69BB40->5BF450 precedes the lower load, including failure.
    let wall_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
    crate::app::radar_movie::retire(state, wall_ms);
    crate::app::chrono_screen::retire(state, wall_ms);
    use crate::app::persistence::{PreparedLoadError, PreparedSavedGame, SavedGamePreparationView};
    let from_menu = state.frontend.screen == crate::ui::game_screen::GameScreen::MainMenu;
    crate::app::loading::pump::present_saved_game_loading(state);
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
                    .as_ref()
                    .map(|runtime| &runtime.simulation),
                frontend_main: &state.frontend.frontend_main_rng,
                rules_owner,
                tile_cache: &state.process_assets.tile_variant_selector_cache,
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
        // Main52E619 initializes Scenario/Main only on this route; in-game
        // options resume never enters it. The ordinary offline clock owner
        // keeps Windows GetTickCount distinct from the non-Windows fallback.
        let seed = crate::app::match_bootstrap::read_match_seed(
            &mut crate::app::match_bootstrap::OrdinaryMatchSeedClock,
        );
        state
            .match_state
            .sim_runtime
            .as_mut()
            .expect("saved world committed")
            .simulation
            .initialize_random_number_system(seed.value);
    }
    Ok(())
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
        PreparedLoadError::MissingRules => log::warn!("Load: {error}"),
        #[cfg(test)]
        PreparedLoadError::MissingCurrentSimulation
        | PreparedLoadError::MissingMapHash
        | PreparedLoadError::MissingTerrainTemplate
        | PreparedLoadError::MissingOverlayRegistry => log::error!("Load: {error}"),
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
        | PreparedLoadError::LocalView(_) => {
            log::error!("Load: {error}")
        }
    }
}

#[cfg(test)]
mod save_name_tests {
    use super::sanitize_save_name;
    use crate::sim::snapshot::GameSnapshot;
    use crate::sim::world::Simulation;

    #[test]
    fn empty_returns_empty() {
        assert_eq!(sanitize_save_name(""), "");
        assert_eq!(sanitize_save_name("   "), "");
        assert_eq!(sanitize_save_name("\t\n"), "");
    }

    #[test]
    fn strips_path_separators() {
        assert_eq!(sanitize_save_name("../foo"), ".._foo");
        assert_eq!(sanitize_save_name("a/b\\c"), "a_b_c");
    }

    #[test]
    fn strips_windows_reserved_chars() {
        assert_eq!(sanitize_save_name("a:b*c?d\"e<f>g|h"), "a_b_c_d_e_f_g_h");
    }

    #[test]
    fn keeps_normal_chars() {
        assert_eq!(sanitize_save_name("miner stuck repro"), "miner stuck repro");
        assert_eq!(sanitize_save_name("dock_fix_a"), "dock_fix_a");
    }

    #[test]
    fn caps_at_64_chars() {
        let long: String = "x".repeat(100);
        let out = sanitize_save_name(&long);
        assert_eq!(out.len(), 64);
    }

    #[test]
    fn trims_whitespace() {
        assert_eq!(sanitize_save_name("  hello  "), "hello");
    }

    #[test]
    fn gsi_17_02_unicode_filename_cap_preserves_exact_envelope_description() {
        let raw_description = "保存".repeat(40);
        let filename_part = sanitize_save_name(&raw_description);
        assert_eq!(filename_part.chars().count(), 64);

        let mut sim = Simulation::new();
        sim.session.map_name = "OFFICIAL.MAP".to_string();
        let bytes = GameSnapshot::save_validated(&sim, 1, 2, &raw_description, 3);
        let header = GameSnapshot::read_header(&bytes).expect("current VERA header");
        assert_eq!(header.description, raw_description);
    }
}
