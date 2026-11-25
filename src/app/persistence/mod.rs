//! Save-game repository, transactional restore, and process-lifetime persistence state.
//!
//! The repository owns every save-directory filesystem operation. Quickload
//! selects the newest save by filesystem modification time.
//!
//! F12 owner tree: options persistence lives here beside the save repository.

pub(crate) mod account;
pub(crate) mod autosave;
#[cfg(any(target_os = "android", test))]
pub(crate) mod chat_hint;
pub(crate) mod commands;
pub(crate) mod intro_play;
pub(crate) mod options;
pub(crate) mod options_profile;
#[cfg(feature = "phobos")]
pub(crate) mod phobos_settings;
pub(crate) mod profile_file;
mod resource_load;
pub(crate) mod rules_profile;
mod save_migration;
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
pub(crate) mod startup_backend;
pub(crate) mod wol_profile;
pub(crate) use resource_load::{PreparedSavedGame, SavedGamePreparationView};

#[cfg(any(feature = "dev-overlays", test))]
use crate::util::time::SystemTime;
use std::path::{Path, PathBuf};

use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::ruleset::RuleSet;
use crate::sim::snapshot::{
    GameSnapshot, GameSnapshotHeader, SnapshotError, SnapshotMapRestoreOutput, SnapshotRestoreError,
};
use crate::sim::world::Simulation;

#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
const DEFAULT_SAVES_DIRECTORY: &str = "saves";
#[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
const DEFAULT_SAVES_DIRECTORY: &str = "/saves";

/// Process-lifetime save-game state owned by the app persistence domain.
pub(crate) struct PersistenceState {
    /// MainTick55DBCD's process-global ABCE08. It is deliberately outside
    /// the saved Scenario and survives a same-mission restart.
    initial_campaign_save_attempted: bool,
    /// The one UI load awaiting its loading frame and subsequent redraw.
    pub(crate) pending_load: Option<commands::PendingSavedGameLoad>,
    /// The process-lifetime retail Options/Video/Audio profile. UI, window,
    /// render, and audio state are projections of this single retained value.
    pub(crate) options_profile: options_profile::RetailOptionsProfile,
    pub(crate) repository: SaveRepository,
    /// The opt-in autosave's schedule (`[VERA] AutosaveMinutes=`).
    pub(crate) autosave: autosave::Autosave,
}

impl PersistenceState {
    pub(crate) fn new(options_profile: options_profile::RetailOptionsProfile) -> Self {
        Self {
            initial_campaign_save_attempted: false,
            pending_load: None,
            options_profile,
            repository: SaveRepository::new(),
            autosave: autosave::Autosave::default(),
        }
    }

    /// CampaignDialog52F28B and successful MapSelect5AE126 arm one attempt.
    pub(crate) fn arm_initial_campaign_save(&mut self) {
        self.initial_campaign_save_attempted = false;
    }

    /// Successful campaign content load67F7C1 suppresses a new initial save.
    pub(crate) fn loaded_campaign(&mut self) {
        self.initial_campaign_save_attempted = true;
    }

    /// 55DBCD..55DC92 stores the latch before IO; failure must not retry each frame.
    fn take_initial_campaign_save(&mut self, campaign: bool) -> bool {
        if !campaign || self.initial_campaign_save_attempted {
            return false;
        }
        self.initial_campaign_save_attempted = true;
        true
    }

    pub(crate) fn initial_campaign_save_pending(&self, campaign: bool) -> bool {
        campaign && !self.initial_campaign_save_attempted
    }

    /// `[VERA] HealthBars=`: the client health-strip policy. Presentation only;
    /// the simulation never reads it.
    pub(crate) fn health_bar_mode(&self) -> crate::ui::main_menu_dialogs::options::HealthBarMode {
        self.options_profile.health_bars
    }

    /// `[VERA] ColorVisionMode=`: the final-frame colour filter. Presentation
    /// only; the simulation never reads it, and the frame reads it live.
    pub(crate) fn color_vision_mode(
        &self,
    ) -> crate::ui::main_menu_dialogs::options::ColorVisionMode {
        self.options_profile.color_vision
    }

    /// This frame's matrix for the shared frame-output filter.
    pub(crate) fn color_vision_matrix(&self) -> Option<crate::render::color_vision::Mat3> {
        crate::app::state::color_vision_matrix_for(self.color_vision_mode())
    }

    /// Whether VERA's HD radar is on (`[VERA] HdRadar=`); a capture takes its
    /// profile's choice (`PlatformState::capture_choices`).
    pub(crate) fn hd_radar(
        &self,
        capture: Option<&crate::app::state::CapturePresentation>,
    ) -> bool {
        capture.map_or(self.options_profile.hd_radar, |capture| capture.hd_radar)
    }

    /// Whether VERA's smooth sidebar is on (`[VERA] SmoothSidebar=`); a
    /// capture takes its profile's choice.
    pub(crate) fn smooth_sidebar(
        &self,
        capture: Option<&crate::app::state::CapturePresentation>,
    ) -> bool {
        capture.map_or(self.options_profile.smooth_sidebar, |capture| {
            capture.smooth_sidebar
        })
    }

    /// The Sidebar size the match frame uses; a capture takes its own,
    /// Normal unless its profile says.
    pub(crate) fn sidebar_size(
        &self,
        capture: Option<&crate::app::state::CapturePresentation>,
    ) -> crate::ui::main_menu_dialogs::options::SidebarSize {
        capture.map_or(self.options_profile.sidebar, |capture| capture.sidebar)
    }

    /// Whether the setup screens show VERA's own map pictures
    /// (`[VERA] HdMapPreviews=`, `app::map_picture`). Captures keep the map's
    /// `[PreviewPack]` picture, which retail comparisons check, and the
    /// browser build cannot start the hidden run that draws one.
    pub(crate) fn hd_map_previews(
        &self,
        capture: Option<&crate::app::state::CapturePresentation>,
    ) -> bool {
        crate::ui::main_menu_dialogs::options::HAS_HD_MAP_PREVIEWS
            && capture.is_none()
            && self.options_profile.hd_map_previews
    }

    /// Whether VERA's online client draws its enlarged art smoothed
    /// (`[VERA] SmoothOnlineMenus=`). The native-UI diagnostics, the only
    /// sessions without that client, keep their art's own pixels.
    pub(crate) fn smooth_online_menus(&self, native_shell_ui: bool) -> bool {
        self.options_profile.smooth_online_menus && !native_shell_ui
    }

    /// A one-shot production capture uses its own new directory, so the
    /// ordinary save command cannot replace a player's existing saves.
    pub(crate) fn isolate_capture_saves(&mut self, directory: &Path) -> std::io::Result<()> {
        if self.repository.directory == directory {
            return Ok(());
        }
        std::fs::create_dir(directory)?;
        self.repository = SaveRepository::at(directory);
        Ok(())
    }
}

/// Every failure that can prevent an in-scenario snapshot from being committed.
#[derive(Debug, thiserror::Error)]
pub(crate) enum PreparedLoadError {
    #[error("could not read save file: {0}")]
    ReadFile(#[source] std::io::Error),
    #[error("a network match cannot load a saved game")]
    NetworkMatch,
    // The three `#[cfg(test)]` variants belong to the headless same-content
    // fixture route; saved-resource preparation rebuilds those inputs.
    #[cfg(test)]
    #[error("no active simulation to restore")]
    MissingCurrentSimulation,
    #[cfg(test)]
    #[error("active world has no authoritative source-map digest")]
    MissingMapHash,
    #[error("active rules are unavailable")]
    MissingRules,
    #[cfg(test)]
    #[error("no resolved_terrain available")]
    MissingTerrainTemplate,
    #[error(transparent)]
    Snapshot(#[from] SnapshotError),
    #[error(transparent)]
    Restore(SnapshotRestoreError),
    #[error(transparent)]
    FactoryState(#[from] crate::sim::production::FactoryRestoreError),
    #[error("invalid saved sidebar order: {0}")]
    SidebarOrder(String),
    #[error("invalid saved tactical view: {0}")]
    TacticalView(String),
    #[error("saved game resources are unavailable: {0}")]
    Resources(String),
    #[error(transparent)]
    Rules(#[from] crate::rules::error::RulesError),
    #[error(transparent)]
    Terrain(#[from] crate::map::resolved_terrain::SavedTerrainError),
    #[error(transparent)]
    LocalView(#[from] crate::sim::snapshot::saved_local_view::SavedLocalViewError),
    #[error(transparent)]
    RadarEvents(#[from] crate::render::radar::SavedRadarEventsError),
}

impl From<SnapshotRestoreError> for PreparedLoadError {
    fn from(error: SnapshotRestoreError) -> Self {
        match error {
            SnapshotRestoreError::Factory(source) => Self::FactoryState(source),
            other => Self::Restore(other),
        }
    }
}

/// Fully validated, cache-rebuilt replacement state ready for one infallible commit.
pub(crate) struct PreparedLoad {
    simulation: Simulation,
    map_restore: SnapshotMapRestoreOutput,
    sinking_waterlines: Vec<(u64, i16)>,
    anim_intensities: Vec<(u64, i32)>,
    tactical_view: Option<crate::sim::snapshot::SavedTacticalView>,
    sidebar_order: Option<crate::app::sidebar_projection::PreparedSidebarOrder>,
}

/// Presentation changes released only after the validated world commits.
pub(crate) struct CommittedLoadPresentation {
    pub(crate) occupied_overlays: Vec<crate::map::overlay::OverlayEntry>,
    pub(crate) sinking_waterlines: Vec<(u64, i16)>,
    pub(crate) anim_intensities: Vec<(u64, i32)>,
    pub(crate) tactical_view: Option<crate::sim::snapshot::SavedTacticalView>,
    pub(crate) sidebar_order: Option<crate::app::sidebar_projection::PreparedSidebarOrder>,
}

/// Fixture adapter for snapshots without a production resource record.
///
/// Preparation can inspect only these references. App-owned screen, pacing,
/// diagnostics, presentation, lighting, and panel state therefore remain
/// structurally outside the fallible part of the transaction.
#[cfg(test)]
pub(crate) struct LoadPreparationView<'a> {
    repository: &'a SaveRepository,
    current_simulation: Option<&'a Simulation>,
    expected_map_hash: Option<u64>,
    rules: Option<&'a RuleSet>,
    terrain_template: Option<&'a ResolvedTerrainGrid>,
}

#[cfg(test)]
impl<'a> LoadPreparationView<'a> {
    /// Production inputs come from the one bound runtime, never shell fallback
    /// rules or a registry/terrain borrowed from a different match.
    pub(crate) fn from_runtime(
        repository: &'a SaveRepository,
        runtime: Option<&'a crate::sim::runtime::SimRuntime>,
        expected_map_hash: Option<u64>,
    ) -> Self {
        Self {
            repository,
            current_simulation: runtime.map(|runtime| &runtime.simulation),
            expected_map_hash,
            rules: runtime.map(|runtime| &runtime.resources.rules),
            terrain_template: runtime
                .and_then(|runtime| runtime.resources.terrain_template.as_ref()),
        }
    }

    // Fault injection for independently missing historical restore inputs.
    #[cfg(test)]
    fn new(
        repository: &'a SaveRepository,
        current_simulation: Option<&'a Simulation>,
        expected_map_hash: Option<u64>,
        rules: Option<&'a RuleSet>,
        terrain_template: Option<&'a ResolvedTerrainGrid>,
    ) -> Self {
        Self {
            repository,
            current_simulation,
            expected_map_hash,
            rules,
            terrain_template,
        }
    }
}

impl PreparedLoad {
    /// Read and prepare a save while holding only immutable references.
    /// Startup admission is match-owned and outside this transaction.
    #[cfg(test)]
    pub(crate) fn from_repository(
        view: LoadPreparationView<'_>,
        path: &Path,
    ) -> Result<Self, PreparedLoadError> {
        let bytes = view
            .repository
            .read(path)
            .map_err(PreparedLoadError::ReadFile)?;
        Self::prepare_candidate(
            &bytes,
            view.current_simulation,
            view.expected_map_hash,
            view.rules,
            view.terrain_template,
        )
    }

    /// Perform every fallible validation and restoration step against owned
    /// candidate state. The live simulation is borrowed only to retain the
    /// process-global seed/Main/MapGen continuation and skipped cache inputs.
    #[cfg(test)]
    fn prepare_candidate(
        bytes: &[u8],
        current_simulation: Option<&Simulation>,
        expected_map_hash: Option<u64>,
        rules: Option<&RuleSet>,
        terrain_template: Option<&ResolvedTerrainGrid>,
    ) -> Result<Self, PreparedLoadError> {
        let current_simulation =
            current_simulation.ok_or(PreparedLoadError::MissingCurrentSimulation)?;
        let expected_map_hash = expected_map_hash.ok_or(PreparedLoadError::MissingMapHash)?;
        let rules = rules.ok_or(PreparedLoadError::MissingRules)?;
        let snapshot = GameSnapshot::load_validated(
            bytes,
            expected_map_hash,
            rules.simulation_config_hash(),
            current_simulation.session().map_name(),
        )?;
        let terrain_template = terrain_template
            .cloned()
            .ok_or(PreparedLoadError::MissingTerrainTemplate)?;

        Self::prepare_snapshot(
            snapshot,
            crate::sim::world::SnapshotProcessState::Live(current_simulation),
            rules,
            terrain_template,
            *current_simulation.terrain_speed_config(),
            crate::sim::snapshot::PlayerSaveCells::FixtureBaseline,
        )
    }

    /// One world restore for production resource loads and headless fixtures.
    /// The caller supplies the incoming bound resources and only lends the
    /// current process RNG owners; no fresh scenario construction is replayed.
    fn prepare_snapshot(
        mut snapshot: GameSnapshot,
        process: crate::sim::world::SnapshotProcessState<'_>,
        rules: &RuleSet,
        terrain_template: ResolvedTerrainGrid,
        terrain_speed_config: crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig,
        cells: crate::sim::snapshot::PlayerSaveCells<'_, '_>,
    ) -> Result<Self, PreparedLoadError> {
        let saved_sidebar_order = snapshot.sidebar_order().cloned();
        let tactical_view = snapshot.tactical_view().cloned();
        if let Some(saved) = tactical_view.as_ref() {
            crate::app::input::camera::validate_saved_tactical_view(saved)
                .map_err(PreparedLoadError::TacticalView)?;
        }
        let mut sinking_waterlines = std::mem::take(&mut snapshot.sinking_waterlines);
        let mut anim_intensities = std::mem::take(&mut snapshot.anim_intensities);
        let (simulation, map_restore) = snapshot.restore_player_world(
            process,
            rules,
            terrain_template,
            terrain_speed_config,
            cells,
        )?;

        // A render cache can retain an outgoing ID until its next sweep. Only
        // restored entity identities may re-enter the new presentation owner;
        // do not infer +3CA from current height or from the sinking byte.
        sinking_waterlines.retain(|(id, _)| simulation.entities().contains(*id));
        anim_intensities.retain(|(id, _)| simulation.anim(*id).is_some());
        // MouseLoad5BDF70 preserves the four strip lists; no-init5BE9B0
        // resets their progress only. Hydration is fallible and borrows no
        // outgoing projection, so a rejected save cannot replace its order.
        let sidebar_order = crate::app::sidebar_projection::prepare_saved_order(
            saved_sidebar_order,
            &simulation,
            rules,
        )
        .map_err(PreparedLoadError::SidebarOrder)?;
        Ok(Self {
            simulation,
            map_restore,
            sinking_waterlines,
            anim_intensities,
            sidebar_order,
            tactical_view,
        })
    }

    pub(crate) fn native_tiberium_stats(
        &self,
    ) -> crate::sim::ore_growth::NativeTiberiumRebuildStats {
        self.map_restore.native_tiberium_stats
    }

    /// Publish prepared dummy fields onto the retained process identity. Both
    /// fixture and production commits pass through this single world handoff.
    fn into_committed(
        mut self,
        live_dummy: crate::map::resolved_terrain::SharedCellDummy,
        process_main: &crate::sim::rng::MainRng,
    ) -> (Simulation, CommittedLoadPresentation) {
        live_dummy.adopt_prepared_load_state(&self.simulation.effective_shared_cell_dummy());
        self.simulation.bind_shared_cell_dummy(live_dummy);
        self.simulation.install_fresh_process_main(process_main);
        // Original successful LoadGame67E6C4 publishes A8ED5C=1 after the
        // fallible content/read tail. Failed preparation never changes it.
        self.simulation.publish_scenario_started();
        // A loaded match draws its turns as a new match does
        // (`world::turn_streams`): a save does not carry the switch.
        self.simulation.separate_turn_streams(true);
        (
            self.simulation,
            CommittedLoadPresentation {
                occupied_overlays: self.map_restore.occupied_overlays,
                sinking_waterlines: self.sinking_waterlines,
                anim_intensities: self.anim_intensities,
                sidebar_order: self.sidebar_order,
                tactical_view: self.tactical_view,
            },
        )
    }

    #[cfg(test)]
    pub(crate) fn commit_into(
        self,
        runtime: &mut crate::sim::runtime::SimRuntime,
    ) -> CommittedLoadPresentation {
        // These fixtures keep no app process cell: the install writes a
        // stand-in one that nothing reads.
        let (simulation, presentation) = self.into_committed(
            runtime.simulation.effective_shared_cell_dummy(),
            &crate::sim::rng::MainRng::new(0),
        );
        runtime.replace_simulation(simulation);
        presentation
    }
}

/// One valid row in the save-game list.
pub(crate) struct SaveEntry {
    pub(crate) path: PathBuf,
    pub(crate) header: GameSnapshotHeader,
}

/// Filesystem stage that failed while writing a named save. Only the
/// `dev-overlays` save log reads it.
#[cfg(any(feature = "dev-overlays", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SaveWriteStage {
    CreateDirectory,
    WriteFile,
}

/// Save write failure retaining the old caller-visible stage distinction.
#[derive(Debug)]
pub(crate) struct SaveWriteError {
    #[cfg(any(feature = "dev-overlays", test))]
    stage: SaveWriteStage,
    source: std::io::Error,
}

impl SaveWriteError {
    fn write_file(source: std::io::Error) -> Self {
        Self {
            #[cfg(any(feature = "dev-overlays", test))]
            stage: SaveWriteStage::WriteFile,
            source,
        }
    }

    #[cfg(any(feature = "dev-overlays", test))]
    pub(crate) fn stage(&self) -> SaveWriteStage {
        self.stage
    }
}

impl std::fmt::Display for SaveWriteError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.source.fmt(formatter)
    }
}

impl std::error::Error for SaveWriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// Concrete owner of the save directory and every operation performed in it.
pub(crate) struct SaveRepository {
    directory: PathBuf,
}

impl SaveRepository {
    fn new() -> Self {
        Self {
            directory: PathBuf::from(DEFAULT_SAVES_DIRECTORY),
        }
    }

    pub(crate) fn at(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    #[cfg(any(feature = "dev-overlays", test))]
    pub(crate) fn directory(&self) -> &Path {
        &self.directory
    }

    pub(crate) fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        crate::util::file_system::read(path)
    }

    /// A slot's display description is stored verbatim. Only its generated
    /// filename trims whitespace, replaces reserved characters and caps length.
    fn filename_part(description: &str) -> Option<String> {
        let trimmed = description.trim();
        if trimmed.is_empty() {
            return None;
        }
        Some(
            trimmed
                .chars()
                .take(64)
                .map(|ch| match ch {
                    '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
                    ch if ch.is_control() => '_',
                    ch => ch,
                })
                .collect(),
        )
    }

    /// Shared destination policy for ordinary saves and converted saves.
    fn write_player_slot(
        &self,
        overwrite: Option<&Path>,
        filename_part: Option<&str>,
        tick: u64,
        timestamp: u64,
        bytes: &[u8],
    ) -> Result<PathBuf, SaveWriteError> {
        if let Some(path) = overwrite {
            self.overwrite_existing(path, bytes)?;
            return Ok(path.to_owned());
        }
        let filename = match filename_part {
            Some(name) => format!("save_{name}_tick{tick}_{timestamp}.bin"),
            None => format!("save_tick{tick}_{timestamp}.bin"),
        };
        self.write_named(&filename, bytes)
    }

    pub(crate) fn write_named(
        &self,
        file_name: &str,
        bytes: &[u8],
    ) -> Result<PathBuf, SaveWriteError> {
        crate::util::file_system::create_dir_all(&self.directory).map_err(|source| {
            SaveWriteError {
                #[cfg(any(feature = "dev-overlays", test))]
                stage: SaveWriteStage::CreateDirectory,
                source,
            }
        })?;
        let path = self.directory.join(file_name);
        crate::util::file_system::write_new(&path, bytes).map_err(SaveWriteError::write_file)?;
        Ok(path)
    }

    /// The autosave slot (`autosave::FILE_NAME`): created on the first
    /// autosave, then replaced through [`Self::overwrite_existing`]'s staged
    /// rename, so a failed write leaves the previous autosave.
    pub(crate) fn write_autosave(&self, bytes: &[u8]) -> Result<PathBuf, SaveWriteError> {
        let path = self.directory.join(autosave::FILE_NAME);
        if crate::util::file_system::exists(&path) {
            self.overwrite_existing(&path, bytes)?;
            Ok(path)
        } else {
            self.write_named(autosave::FILE_NAME, bytes)
        }
    }

    /// New saves may not silently replace a timestamp-name collision. Remove
    /// only our newly created partial file if writing fails.
    fn create_new_file(&self, path: &Path, bytes: &[u8]) -> Result<(), SaveWriteError> {
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        {
            crate::util::file_system::write_new(path, bytes).map_err(SaveWriteError::write_file)
        }
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .map_err(SaveWriteError::write_file)?;
            let result = file.write_all(bytes).and_then(|()| file.sync_all());
            drop(file);
            if let Err(source) = result {
                let _ = crate::util::file_system::remove_file(path);
                return Err(SaveWriteError::write_file(source));
            }
            Ok(())
        }
    }

    /// Only an explicit existing repository row can be replaced. Stage the
    /// complete snapshot beside it, then publish with same-directory rename;
    /// failure leaves the previous save intact.
    pub(crate) fn overwrite_existing(
        &self,
        path: &Path,
        bytes: &[u8],
    ) -> Result<(), SaveWriteError> {
        if !self.contains_save_path(path) {
            return Err(SaveWriteError::write_file(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "overwrite target is not a save in this repository",
            )));
        }
        self.replace_file(path, bytes, GameSnapshotHeader::is_compatible)
    }

    fn replace_file(
        &self,
        path: &Path,
        bytes: &[u8],
        eligible: impl FnOnce(&GameSnapshotHeader) -> bool,
    ) -> Result<(), SaveWriteError> {
        // Re-read the selected row immediately before publishing. It may
        // have changed while the dialog was open or the snapshot was staged.
        let validate = || {
            if eligible(&Self::file_header(path)?) {
                Ok(())
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "the selected save is no longer compatible",
                ))
            }
        };
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        {
            validate().map_err(SaveWriteError::write_file)?;
            crate::util::file_system::write(path, bytes).map_err(SaveWriteError::write_file)
        }
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        {
            // A process-unique serial for staging file names.
            static NEXT_STAGE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let directory = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            let stage = loop {
                let id = NEXT_STAGE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let stage = directory.join(format!(".save-{}-{id}.tmp", std::process::id()));
                match self.create_new_file(&stage, bytes) {
                    Ok(()) => break stage,
                    Err(error) if error.source.kind() == std::io::ErrorKind::AlreadyExists => {
                        continue;
                    }
                    Err(error) => return Err(error),
                }
            };
            let result = validate().and_then(|()| std::fs::rename(&stage, path));
            if result.is_err() {
                let _ = crate::util::file_system::remove_file(&stage);
            }
            result.map_err(SaveWriteError::write_file)
        }
    }

    /// The game's one retained initial save per mission. Native67CF13..67CF42
    /// forwards the entire filename to StgCreateDocfile(CREATE), wherever it
    /// points. VERA: the name is the mission's map name, which a mod, a map or
    /// a saved game supplies, so only a logical name is written, under the save
    /// root; a name that would leave it (a drive, a root, `..`) is refused.
    /// Missing subdirectories fail, and an existing VERA automatic slot is replaced.
    /// The Rust-format boundary preserves foreign/native SAV files rather than
    /// replacing them with an incompatible snapshot. The attempt still consumes
    /// the native latch; this does not claim native SAV compatibility.
    /// User saves keep their separate create-new/explicit-overwrite policy.
    pub(crate) fn write_initial_campaign(
        &self,
        filename: &str,
        bytes: &[u8],
    ) -> Result<PathBuf, SaveWriteError> {
        let file = Path::new(filename);
        if !crate::map::source::is_logical_name(filename)
            || file.extension().and_then(|ext| ext.to_str()) != Some("sav")
        {
            return Err(SaveWriteError::write_file(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "initial campaign save must name a .sav file in the save folder",
            )));
        }
        crate::util::file_system::create_dir_all(&self.directory).map_err(|source| {
            SaveWriteError {
                #[cfg(any(feature = "dev-overlays", test))]
                stage: SaveWriteStage::CreateDirectory,
                source,
            }
        })?;
        let path = self.directory.join(file);
        if crate::util::file_system::exists(&path) {
            // SaveGame67CF42 can replace this mission's own older VERA slot.
            self.replace_file(&path, bytes, |_| true)?;
        } else {
            self.create_new_file(&path, bytes)?;
        }
        Ok(path)
    }

    fn is_save_file(path: &Path) -> bool {
        matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("bin" | "sav")
        )
    }

    fn contains_save_path(&self, path: &Path) -> bool {
        path.parent() == Some(self.directory.as_path()) && Self::is_save_file(path)
    }

    pub(crate) fn delete(&self, path: &Path) -> std::io::Result<()> {
        crate::util::file_system::remove_file(path)
    }

    pub(crate) fn exists(&self, path: &Path) -> bool {
        crate::util::file_system::exists(&path)
    }

    /// Native5596A0 browser metadata with each file's write time. The shared
    /// browser sorts these records with the established retail comparator after
    /// adding New.
    pub(crate) fn browser_entries(&self) -> Vec<(SaveEntry, u64)> {
        self.readable_entries().collect()
    }

    /// Whether any save reads: the scan stops at the first one, like
    /// `LoadOptionsClass__HasLoadableSave` `0x00559C20`. VERA-only: a save
    /// another build wrote counts too, because Load lists it (marked, not
    /// loadable) where gamemd hides it.
    pub(crate) fn has_browser_entry(&self) -> bool {
        self.readable_entries().next().is_some()
    }

    fn file_header(path: &Path) -> std::io::Result<GameSnapshotHeader> {
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        let reader = std::fs::File::open(path)?;
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        let reader = std::io::Cursor::new(crate::util::file_system::read_shared(path)?);
        Self::read_browser_metadata(reader)
    }

    /// SaveInfo681840/559ED0 needs metadata, not the simulation body. Reuse
    /// the one snapshot parser with a growing prefix: strings can exceed a
    /// fixed page, but ordinary headers must not read a 200 MB saved world.
    fn read_browser_metadata(
        mut reader: impl std::io::Read,
    ) -> std::io::Result<GameSnapshotHeader> {
        use std::io::Read;
        let mut prefix = Vec::new();
        let mut next = 64;
        loop {
            let added = reader.by_ref().take(next).read_to_end(&mut prefix)?;
            match GameSnapshot::read_metadata(&prefix) {
                Ok(header) => return Ok(header),
                Err(SnapshotError::DeserializeFailed(
                    crate::util::save_codec::Error::UnexpectedEnd,
                )) if added != 0 => next = prefix.len() as u64,
                Err(error) => {
                    return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, error));
                }
            }
        }
    }

    /// Saves whose header reads, with their file time, in directory order.
    fn readable_entries(&self) -> impl Iterator<Item = (SaveEntry, u64)> + '_ {
        crate::util::file_system::read_dir(&self.directory)
            .into_iter()
            .flatten()
            .filter_map(|item| {
                let item = item.ok()?;
                let path = item.path();
                if !Self::is_save_file(&path) {
                    return None;
                }
                let metadata = item.metadata().ok()?;
                if !metadata.is_file() {
                    return None;
                }
                let header = Self::file_header(&path).ok()?;
                let ticks = crate::map::rmg::saved_seeds::system_time_to_file_time(
                    metadata.modified().ok()?,
                );
                Some((SaveEntry { path, header }, ticks))
            })
    }

    /// Quickload policy: select the `.bin` file with the newest filesystem
    /// modification time without reading or validating its snapshot header.
    #[cfg(any(feature = "dev-overlays", test))]
    pub(crate) fn quickload_path_by_modified_time(&self) -> Option<PathBuf> {
        let directory = crate::util::file_system::read_dir(&self.directory).ok()?;
        newest_modified_path(directory.filter_map(|item| {
            let item = item.ok()?;
            let path = item.path();
            if !Self::is_save_file(&path) {
                return None;
            }
            Some((path, item.metadata().ok()?.modified().ok()?))
        }))
    }
}

#[cfg(any(feature = "dev-overlays", test))]
fn newest_modified_path(
    candidates: impl Iterator<Item = (PathBuf, SystemTime)>,
) -> Option<PathBuf> {
    candidates
        .max_by_key(|(_, modified)| *modified)
        .map(|(path, _)| path)
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;
    use crate::app::match_runtime::frame_pacer::LocalFramePacer;
    use crate::app::match_runtime::startup::MatchStartup;
    use crate::map::lighting::CellLightGrid;
    use crate::map::overlay::OverlayEntry;
    use crate::map::resolved_terrain::{DynamicTerrainCellState, ResolvedTerrainCell};
    use crate::rules::ini_parser::IniFile;
    use crate::rules::terrain_rules::SpeedCostProfile;
    use crate::sim::overlay_grid::OverlayGrid;
    use crate::sim::replay::{ReplayHeader, ReplayLog};
    use crate::sim::world::{Simulation, SimulationRngState};
    use crate::skirmish_launch::{
        AiDifficulty, LaunchCountry, LaunchStartPosition, LaunchTeam, SkirmishAiSlot,
        SkirmishLaunchMode, SkirmishLaunchOptions, SkirmishLaunchSession, SkirmishLocalSlot,
    };
    use crate::ui::game_screen::GameScreen;

    const LOAD_FIXTURE_MAP_HASH: u64 = 0x1234_5678_9ABC_DEF0;
    const LOAD_FIXTURE_MAP_NAME: &str = "TRANSACTION.MAP";
    const LOAD_FIXTURE_SEED: u32 = 0x1234_5678;

    struct TestClock(u32);

    impl crate::app::match_bootstrap::MatchSeedClock for TestClock {
        fn low_u32(&mut self) -> u32 {
            self.0
        }

        fn source(&self) -> crate::app::match_bootstrap::MatchSeedSource {
            crate::app::match_bootstrap::MatchSeedSource::Controlled
        }

        fn seed_authority_certifying(&self) -> bool {
            true
        }
    }

    fn load_fixture_launch() -> SkirmishLaunchSession {
        SkirmishLaunchSession::from_rows(
            SkirmishLaunchMode {
                class: crate::skirmish_modes::MpModeClass::Battle,
                id: 1,
                ui_name_key: "GUI:Battle".into(),
                tooltip_key: "STT:ModeBattle".into(),
                override_file: "MPBattleMD.ini".into(),
                map_filter: "standard".into(),
                random_maps_allowed: true,
                allies_allowed: true,
                must_ally: false,
            },
            Some(LOAD_FIXTURE_MAP_NAME.into()),
            vec![crate::skirmish_launch::SkirmishHumanSlot {
                raw_slot_index: Some(0),
                name: "Player".into(),
                slot: SkirmishLocalSlot {
                    country: LaunchCountry::America,
                    country_random: false,
                    color_index: 0,
                    color_random: false,
                    start_position: LaunchStartPosition::Position(0),
                    team: LaunchTeam::None,
                },
                role: crate::skirmish_launch::LaunchHumanRole::from_observer(false),
            }],
            0,
            vec![SkirmishAiSlot {
                raw_slot_index: 1,
                country: LaunchCountry::Russia,
                country_random: false,
                color_index: 1,
                color_random: false,
                start_position: LaunchStartPosition::Position(1),
                team: LaunchTeam::None,
                difficulty: AiDifficulty::Easy,
            }],
            SkirmishLaunchOptions::default(),
        )
    }

    fn startup_authority(seed: u32) -> MatchStartup {
        let launch = load_fixture_launch();
        let accepted = match crate::app::match_bootstrap::classify_startup_session(&launch) {
            crate::app::match_bootstrap::StartupSessionClassification::AcceptedExplicitFixedBattle(
                accepted,
            ) => *accepted,
            other => panic!("startup fixture was not accepted: {other:?}"),
        };
        let mut next_correlation = 1;
        let correlation =
            crate::app::match_bootstrap::allocate_match_correlation(&mut next_correlation).unwrap();
        let startup = crate::app::match_bootstrap::prepare_match_startup(
            correlation,
            accepted,
            &mut TestClock(seed),
        );
        let initial_simulation = Simulation::with_seed(u64::from(seed));
        let mut authority = MatchStartup::default();
        authority.begin(Some(correlation));
        authority
            .acknowledge(startup, Some(&initial_simulation), true)
            .unwrap();
        authority
    }

    struct RunningMatchTestState {
        runtime: crate::sim::runtime::SimRuntime,
        /// The app-owned diagnostics slot (F10) — represented here so the
        /// baseline proves a failed load leaves the segment untouched.
        replay_log: Option<ReplayLog>,
        startup: MatchStartup,
        screen: GameScreen,
        frame_pacer: LocalFramePacer,
        overlay_render_index: Vec<OverlayEntry>,
        lighting_grid: CellLightGrid,
        in_game_menu: crate::ui::pause_menu::InGameMenuState,
    }

    impl RunningMatchTestState {
        fn running(rules: &RuleSet) -> Self {
            let simulation = load_fixture_simulation(true);
            let shared_cell_dummy = simulation.effective_shared_cell_dummy();
            shared_cell_dummy.set_level_slope(-7, 11);
            shared_cell_dummy.stamp_coord(7, 9);
            shared_cell_dummy
                .apply_bridge_flag_slot(crate::map::bridge_facts::BridgeStampSlot::Anchor, true);
            let mut replay = ReplayLog::new(ReplayHeader {
                pixel_conversion_bounds: Default::default(),
                version: 1,
                tick_hz: 15,
                seed: simulation.session().seed(),
                map_name: simulation.session().map_name().to_owned(),
                rules_hash: rules.simulation_config_hash(),
            });
            replay.record_tick(1, Vec::new(), Some(simulation.state_hash()));
            let replay_log = Some(replay);

            let startup = startup_authority(LOAD_FIXTURE_SEED);
            let mut frame_pacer = LocalFramePacer::new();
            frame_pacer.record_admitted_frame(32);
            let mut lighting_grid = CellLightGrid::new();
            lighting_grid.set_compat_tint((3, 4), [0.25, 0.5, 0.75]);

            Self {
                runtime: crate::sim::runtime::SimRuntime::from_simulation(simulation),
                replay_log,
                startup,
                screen: GameScreen::InGame,
                frame_pacer,
                overlay_render_index: vec![OverlayEntry {
                    rx: 3,
                    ry: 4,
                    overlay_id: 5,
                    frame: 6,
                }],
                lighting_grid,
                in_game_menu: crate::ui::pause_menu::InGameMenuState::SavedGame(
                    crate::ui::skirmish_shell::SavedSeedMode::Load,
                ),
            }
        }

        fn baseline(&self) -> RunningMatchBaseline {
            RunningMatchBaseline {
                simulation_hash: self.runtime.simulation.state_hash(),
                shared_cell_dummy: self.effective_shared_cell_dummy_snapshot(),
                rng: self.runtime.simulation.rng_state(),
                replay: self.replay_log.as_ref().map(|replay| {
                    (
                        replay.header.seed,
                        replay.ticks.len(),
                        replay.ticks.first().and_then(|tick| tick.state_hash),
                    )
                }),
                startup: self.startup.clone(),
                screen: self.screen.clone(),
                pacer_admits_same_bucket: self.frame_pacer.should_admit(32, 1, None, false),
                pacer_admits_next_bucket: self.frame_pacer.should_admit(48, 1, None, false),
                overlay_render_index: self
                    .overlay_render_index
                    .iter()
                    .map(|entry| (entry.rx, entry.ry, entry.overlay_id, entry.frame))
                    .collect(),
                lighting_tint: self.lighting_grid.tint_or_default((3, 4)),
                in_game_menu: self.in_game_menu,
            }
        }

        fn effective_shared_cell_dummy_snapshot(
            &self,
        ) -> crate::map::resolved_terrain::SharedCellDummySnapshot {
            self.runtime
                .simulation
                .effective_shared_cell_dummy()
                .snapshot()
        }
    }

    #[derive(Debug, PartialEq)]
    struct RunningMatchBaseline {
        simulation_hash: u64,
        shared_cell_dummy: crate::map::resolved_terrain::SharedCellDummySnapshot,
        rng: SimulationRngState,
        replay: Option<(u64, usize, Option<u64>)>,
        startup: MatchStartup,
        screen: GameScreen,
        pacer_admits_same_bucket: bool,
        pacer_admits_next_bucket: bool,
        overlay_render_index: Vec<(u16, u16, u8, u8)>,
        lighting_tint: [f32; 3],
        in_game_menu: crate::ui::pause_menu::InGameMenuState,
    }

    mod animated_tiberium_restore_tests;
    mod building_repair_restore_tests;
    mod cloak_restore_tests;
    mod factory_restore_tests;
    mod infantry_death_restore_tests;
    mod rule_cache_restore_tests;
    mod sidebar_order_restore_tests;
    mod sinking_waterline_restore_tests;

    include!("tube_hierarchy_restore_tests.rs");

    fn load_fixture_rules() -> RuleSet {
        let ini = IniFile::from_str(
            "[Countries]\n0=Americans\n1=Alliance\n2=French\n3=Germans\n\
             4=British\n5=Africans\n6=Arabs\n7=Confederation\n8=Russians\n\
             [InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [Warheads]\n\
             [OverlayTypes]\n",
        );
        RuleSet::from_ini(&ini).expect("transaction fixture rules")
    }

    pub(super) fn load_fixture_simulation(with_overlay_grid: bool) -> Simulation {
        let mut simulation = Simulation::with_seed(u64::from(LOAD_FIXTURE_SEED));
        simulation
            .session_mut_for_test()
            .set_map_name_for_test(LOAD_FIXTURE_MAP_NAME.to_string());
        *simulation.overlay_grid_mut_for_test() = with_overlay_grid.then(|| OverlayGrid::new(0, 0));
        simulation
    }

    fn load_fixture_terrain() -> ResolvedTerrainGrid {
        ResolvedTerrainGrid::from_cells(0, 0, Vec::new())
    }

    fn compatibility_snapshot_cell(tile_index: i32) -> ResolvedTerrainCell {
        ResolvedTerrainCell {
            source_tile_index: tile_index,
            final_tile_index: tile_index,
            zone_type: crate::map::resolved_terrain::zone_class::GROUND,
            ..crate::map::resolved_terrain::test_flat_cell(0, 0)
        }
    }

    fn snapshot_bytes(simulation: &Simulation, rules: &RuleSet) -> Vec<u8> {
        GameSnapshot::save_validated(
            simulation,
            LOAD_FIXTURE_MAP_HASH,
            rules.simulation_config_hash(),
            "transaction fixture",
            1,
        )
    }

    fn assert_transaction_failure(
        state: &RunningMatchTestState,
        repository: &SaveRepository,
        path: &Path,
        expected_map_hash: Option<u64>,
        rules: Option<&RuleSet>,
        terrain_template: Option<&ResolvedTerrainGrid>,
        expected_error: impl FnOnce(&PreparedLoadError) -> bool,
    ) {
        // Capture every represented running-match owner before the production
        // seam performs file I/O or begins candidate restoration.
        let baseline = state.baseline();
        let result = PreparedLoad::from_repository(
            LoadPreparationView::new(
                repository,
                Some(&state.runtime.simulation),
                expected_map_hash,
                rules,
                terrain_template,
            ),
            path,
        );
        let error = match result {
            Ok(_) => panic!("invalid load fixture unexpectedly prepared"),
            Err(error) => error,
        };

        assert!(expected_error(&error), "unexpected error: {error}");
        assert_eq!(state.baseline(), baseline);
    }

    fn snapshot(description: &str, save_timestamp: u64) -> Vec<u8> {
        let mut simulation = Simulation::new();
        simulation
            .session_mut_for_test()
            .set_map_name_for_test("POLICY.MAP".to_string());
        GameSnapshot::save_validated(&simulation, 1, 2, description, save_timestamp)
    }

    fn isolated_directory(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "vera20k-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time after Unix epoch")
                .as_nanos(),
        ))
    }

    #[test]
    fn failed_load_preserves_complete_running_match_transaction() {
        let rules = load_fixture_rules();
        let terrain = load_fixture_terrain();
        let state = RunningMatchTestState::running(&rules);
        let directory = isolated_directory("load-transaction-failures");
        let repository = SaveRepository::at(&directory);

        let valid_saved = load_fixture_simulation(true);
        let valid_bytes = snapshot_bytes(&valid_saved, &rules);
        let valid_path = repository
            .write_named("valid.bin", &valid_bytes)
            .expect("write valid transaction fixture");
        let bad_bytes_path = repository
            .write_named("bad-bytes.bin", b"not a snapshot")
            .expect("write bad-byte transaction fixture");

        let mut invalid_identity = load_fixture_simulation(true);
        invalid_identity.set_next_stable_object_id_for_test(0);
        let invalid_identity_path = repository
            .write_named(
                "invalid-identity.bin",
                &snapshot_bytes(&invalid_identity, &rules),
            )
            .expect("write identity-failure transaction fixture");

        let missing_overlay = load_fixture_simulation(false);
        let missing_overlay_path = repository
            .write_named(
                "missing-map-cache.bin",
                &snapshot_bytes(&missing_overlay, &rules),
            )
            .expect("write map/cache-failure transaction fixture");

        assert_transaction_failure(
            &state,
            &repository,
            &bad_bytes_path,
            Some(LOAD_FIXTURE_MAP_HASH),
            Some(&rules),
            Some(&terrain),
            |error| matches!(error, PreparedLoadError::Snapshot(_)),
        );
        assert_transaction_failure(
            &state,
            &repository,
            &valid_path,
            Some(LOAD_FIXTURE_MAP_HASH ^ 1),
            Some(&rules),
            Some(&terrain),
            |error| {
                matches!(
                    error,
                    PreparedLoadError::Snapshot(SnapshotError::MapMismatch { .. })
                )
            },
        );
        assert_transaction_failure(
            &state,
            &repository,
            &valid_path,
            Some(LOAD_FIXTURE_MAP_HASH),
            None,
            Some(&terrain),
            |error| matches!(error, PreparedLoadError::MissingRules),
        );
        assert_transaction_failure(
            &state,
            &repository,
            &valid_path,
            Some(LOAD_FIXTURE_MAP_HASH),
            Some(&rules),
            None,
            |error| matches!(error, PreparedLoadError::MissingTerrainTemplate),
        );
        assert_transaction_failure(
            &state,
            &repository,
            &invalid_identity_path,
            Some(LOAD_FIXTURE_MAP_HASH),
            Some(&rules),
            Some(&terrain),
            |error| {
                matches!(
                    error,
                    PreparedLoadError::Restore(SnapshotRestoreError::ObjectIdCounterBehind { .. })
                )
            },
        );
        assert_transaction_failure(
            &state,
            &repository,
            &missing_overlay_path,
            Some(LOAD_FIXTURE_MAP_HASH),
            Some(&rules),
            Some(&terrain),
            |error| {
                matches!(
                    error,
                    PreparedLoadError::Restore(
                        SnapshotRestoreError::MissingMapAuthorityComponent {
                            component: "OverlayGrid"
                        }
                    )
                )
            },
        );

        std::fs::remove_dir_all(directory).expect("remove load-transaction fixture directory");
    }

    #[test]
    fn successful_same_content_load_preserves_admission_and_bound_resources() {
        let rules = load_fixture_rules();
        let terrain = load_fixture_terrain();
        let mut state = RunningMatchTestState::running(&rules);
        let directory = isolated_directory("load-transaction-startup");
        let repository = SaveRepository::at(&directory);
        let mut saved = load_fixture_simulation(true);
        saved
            .base_reservations_mut_for_test()
            .reserve(None, 3, 4, 2);
        saved
            .base_reservations_mut_for_test()
            .reserve(None, -1, 0, 5);
        saved
            .session_mut_for_test()
            .install_waypoints(&std::collections::HashMap::from([(
                701,
                crate::map::waypoints::Waypoint {
                    index: 701,
                    rx: 122,
                    ry: 135,
                },
            )]));
        let path = repository
            .write_named("same-content.bin", &snapshot_bytes(&saved, &rules))
            .expect("write same-content transaction fixture");

        let startup_before = state.startup.clone();
        let baseline = state.baseline();
        state.runtime.resources_mut().rules = rules;
        state.runtime.resources_mut().terrain_template = Some(terrain);
        let rules_before = state.runtime.resources.rules.simulation_config_hash();
        let prepared = PreparedLoad::from_repository(
            LoadPreparationView::from_runtime(
                &repository,
                Some(&state.runtime),
                Some(LOAD_FIXTURE_MAP_HASH),
            ),
            &path,
        )
        .unwrap_or_else(|error| panic!("same-content transaction must prepare: {error}"));
        assert_eq!(state.baseline(), baseline);

        let live_dummy = state.runtime.simulation.effective_shared_cell_dummy();
        assert_eq!(live_dummy.snapshot().coord, (7, 9));
        assert_eq!(
            prepared.simulation.base_reservations().raw_mask(None, 3, 4),
            1 << 2,
            "candidate preparation restores real reservation authority verbatim"
        );
        assert_eq!(
            prepared.simulation.base_reservations().dummy_mask(),
            0,
            "raw snapshot decode reconstructs the process-global dummy cleared"
        );
        let runtime = &mut state.runtime;
        let _occupied_overlays = prepared.commit_into(runtime);
        assert_eq!(state.startup, startup_before);
        assert!(state.startup.admits_exact_step());
        assert_eq!(
            runtime.resources.rules.simulation_config_hash(),
            rules_before
        );
        assert!(runtime.resources.terrain_template.is_some());
        assert_eq!(
            runtime.simulation.session().waypoint_cell(701),
            Some((122, 135))
        );
        let simulation = &mut runtime.simulation;
        assert!(
            simulation.turn_streams_separate(),
            "a loaded match draws its turns from their own streams, as a new one does"
        );
        let restored_dummy = simulation.effective_shared_cell_dummy();
        assert!(restored_dummy.same_identity(&live_dummy));
        assert_eq!(
            restored_dummy.snapshot(),
            crate::map::resolved_terrain::SharedCellDummySnapshot {
                coord: (0, 0),
                level: 0,
                slope_type: 0,
                bridge_flags_0x1180: 0,
            },
            "successful load publishes the prepared post-Resize dummy fields"
        );
        assert_eq!(
            simulation.base_reservations().raw_mask(None, 3, 4),
            1 << 2,
            "the narrow dummy reconstruction leaves real reservation state untouched"
        );
        assert_eq!(simulation.base_reservations().dummy_mask(), 0);

        let accepted_hash = simulation.state_hash();
        simulation
            .base_reservations_mut_for_test()
            .reserve(None, -1, 0, 6);
        assert_ne!(simulation.state_hash(), accepted_hash);
        simulation.reconstruct_map_dummy_for_test();
        assert_eq!(simulation.base_reservations().dummy_mask(), 0);
        assert_eq!(
            simulation.state_hash(),
            accepted_hash,
            "with the shared dummy already zero, reconstruction removes only the hashed stale mask"
        );
        let tick_before = runtime.simulation.session().tick();
        runtime
            .advance_frame(
                &[],
                33,
                crate::sim::world::TickLane::Ordinary,
                &mut crate::sim::world::FrameEffects::default(),
            )
            .expect("fixture frame must complete");
        assert_eq!(runtime.simulation.session().tick(), tick_before + 1);
        assert_eq!(state.startup, startup_before);
        assert!(state.startup.admits_exact_step());
        std::fs::remove_dir_all(directory).expect("remove startup fixture directory");
    }

    #[test]
    fn current_house_survives_prepared_load_and_first_headless_frame() {
        let rules = load_fixture_rules();
        let terrain = load_fixture_terrain();
        let mut saved = load_fixture_simulation(true);
        let roster_ini = IniFile::from_str(
            "[Houses]\n0=SavedPlayer\n1=OutgoingPlayer\n[SavedPlayer]\n[OutgoingPlayer]\n[Basic]\nPlayer=SavedPlayer\n",
        );
        let roster = crate::map::houses::parse_house_roster(&roster_ini, &[], Some(&rules));
        crate::sim::scenario_bootstrap::initialize_map_roster_houses(
            &mut saved,
            &roster,
            &rules,
            Some(&roster_ini),
        );
        saved.initialize_campaign_current_house_for_test(&roster_ini);
        let saved_owner = saved.interner().get("SavedPlayer").unwrap();
        let outgoing_owner = saved.interner().get("OutgoingPlayer").unwrap();
        let directory = isolated_directory("current-house-full-load");
        let repository = SaveRepository::at(&directory);
        let path = repository
            .write_named("current-house.bin", &snapshot_bytes(&saved, &rules))
            .expect("write saved House identity");
        let mut current = load_fixture_simulation(true);
        crate::sim::scenario_bootstrap::initialize_map_roster_houses(
            &mut current,
            &roster,
            &rules,
            Some(&roster_ini),
        );
        current.initialize_campaign_current_house_for_test(&IniFile::from_str(
            "[Basic]\nPlayer=OutgoingPlayer\n",
        ));
        assert_eq!(current.session().current_house(), Some(outgoing_owner));
        assert!(!current.houses()[&saved_owner].is_human());
        let mut runtime = crate::sim::runtime::SimRuntime::from_simulation(current);
        runtime.resources_mut().rules = rules;
        runtime.resources_mut().terrain_template = Some(terrain);
        let outgoing_hash = runtime.simulation.state_hash();
        let outgoing_rng = runtime.simulation.rng_state();
        let prepared = PreparedLoad::from_repository(
            LoadPreparationView::from_runtime(
                &repository,
                Some(&runtime),
                Some(LOAD_FIXTURE_MAP_HASH),
            ),
            &path,
        )
        .expect("repository transaction prepares saved House identity");
        assert_eq!(
            runtime.simulation.session().current_house(),
            Some(outgoing_owner)
        );
        assert_eq!(runtime.simulation.state_hash(), outgoing_hash);
        assert_eq!(runtime.simulation.rng_state(), outgoing_rng);
        assert_eq!(
            prepared.simulation.session().current_house(),
            Some(saved_owner)
        );
        let _ = prepared.commit_into(&mut runtime);
        assert_eq!(
            runtime.simulation.session().current_house(),
            Some(saved_owner)
        );
        assert!(runtime.simulation.houses()[&saved_owner].is_human());
        assert!(runtime.simulation.houses()[&saved_owner].player_control());
        assert!(!runtime.simulation.houses()[&outgoing_owner].is_human());
        assert!(!runtime.simulation.houses()[&outgoing_owner].player_control());
        let tick = runtime.simulation.session().tick();
        runtime
            .advance_frame(
                &[],
                1,
                crate::sim::world::TickLane::Ordinary,
                &mut crate::sim::world::FrameEffects::default(),
            )
            .expect("restored current-house frame");
        assert_eq!(runtime.simulation.session().tick(), tick + 1);
        assert_eq!(
            runtime.simulation.session().current_house(),
            Some(saved_owner)
        );
        std::fs::remove_dir_all(directory).expect("remove House load fixture");
    }

    #[test]
    fn current_house_collision_survives_repository_commit_and_headless_frame() {
        let rules = load_fixture_rules();
        let directory = isolated_directory("current-house-collision-load");
        let repository = SaveRepository::at(&directory);
        for (case_index, handle) in ["Neutral", "sPeCiAl", "COMPUTER1"].into_iter().enumerate() {
            let mut session = load_fixture_launch();
            session.local_player_mut().name = handle.to_string();
            let launch =
                crate::sim::scenario_bootstrap::MatchLaunchDescriptor::from_resolved(session)
                    .expect("existing display name remains admitted");
            let mut saved = load_fixture_simulation(true);
            crate::sim::scenario_bootstrap::initialize_skirmish_launch_houses(
                &mut saved,
                &crate::map::houses::HouseRoster::default(),
                &rules,
                &launch,
            );
            let selected = saved.interner().get("Player").unwrap();
            let neutral = saved.interner().get("Neutral").unwrap();
            let ai = saved.interner().get("Computer1").unwrap();
            assert_eq!(saved.session().current_house(), Some(selected));
            assert_ne!(selected, neutral);
            assert_ne!(selected, ai);
            let bytes = snapshot_bytes(&saved, &rules);
            let path = repository
                .write_named(&format!("collision-{case_index}.bin"), &bytes)
                .unwrap();
            let mut outgoing = GameSnapshot::load(&bytes)
                .unwrap()
                .into_simulation_for_test();
            outgoing
                .session_mut_for_test()
                .set_current_house_for_test(Some(ai));
            let mut runtime = crate::sim::runtime::SimRuntime::from_simulation(outgoing);
            runtime.resources_mut().rules = load_fixture_rules();
            runtime.resources_mut().terrain_template = Some(load_fixture_terrain());
            let prepared = PreparedLoad::from_repository(
                LoadPreparationView::from_runtime(
                    &repository,
                    Some(&runtime),
                    Some(LOAD_FIXTURE_MAP_HASH),
                ),
                &path,
            )
            .expect("collision-free saved House identity prepares");
            assert_eq!(runtime.simulation.session().current_house(), Some(ai));
            let _ = prepared.commit_into(&mut runtime);
            assert_eq!(runtime.simulation.session().current_house(), Some(selected));
            assert!(
                runtime.simulation.houses()[&selected].is_human()
                    && runtime.simulation.houses()[&selected].player_control()
            );
            assert!(!runtime.simulation.houses()[&neutral].is_human());
            assert!(!runtime.simulation.houses()[&ai].is_human());
            let tick = runtime.simulation.session().tick();
            runtime
                .advance_frame(
                    &[],
                    1,
                    crate::sim::world::TickLane::Ordinary,
                    &mut crate::sim::world::FrameEffects::default(),
                )
                .expect("restored collision-safe current-house frame");
            assert_eq!(runtime.simulation.session().tick(), tick + 1);
            assert_eq!(runtime.simulation.session().current_house(), Some(selected));
            assert_eq!(launch.session().local_player().name, handle);
        }
        std::fs::remove_dir_all(directory).expect("remove collision load fixture");
    }

    #[test]
    fn gsi_04_02_last_tiles_full_load_route_does_not_retranslate_actual_runtime_ids() {
        let compatibility = crate::map::theater::parse_tileset_ini(
            b"[TileSet0000]\nTilesInSet=3\n\
              [TileSet0001]\nTilesInSet=5\nLastTilesInSet=2\n\
              [TileSet0002]\nTilesInSet=1\n",
            "tem",
        )
        .expect("compatibility table");
        assert_eq!(compatibility.translate_legacy_map_tile_index(5), 8);

        let rules = load_fixture_rules();
        let translated_template =
            ResolvedTerrainGrid::from_cells(1, 1, vec![compatibility_snapshot_cell(8)]);
        let mut runtime_actual = compatibility_snapshot_cell(5);
        runtime_actual.radar_left = [31, 32, 33];

        let mut saved = load_fixture_simulation(false);
        *saved.overlay_grid_mut_for_test() = Some(OverlayGrid::new(1, 1));
        saved.install_resolved_terrain_for_new_map(translated_template.clone());
        saved
            .dynamic_terrain_cells_mut_for_test()
            .record(&runtime_actual);
        let bytes = snapshot_bytes(&saved, &rules);

        let mut current = load_fixture_simulation(false);
        *current.overlay_grid_mut_for_test() = Some(OverlayGrid::new(1, 1));
        let restored = PreparedLoad::prepare_candidate(
            &bytes,
            Some(&current),
            Some(LOAD_FIXTURE_MAP_HASH),
            Some(&rules),
            Some(&translated_template),
        )
        .expect("full persistence prepare/serde/rebuild/restore route")
        .simulation;

        let restored_cell = restored
            .resolved_terrain()
            .and_then(|terrain| terrain.cell(0, 0))
            .expect("restored dynamic terrain cell");
        assert_eq!(
            restored_cell.source_tile_index, 8,
            "the retained already-translated map template is cloned verbatim"
        );
        assert_eq!(
            restored_cell.final_tile_index, 5,
            "serialized runtime actual ID must not cross legacy translation again"
        );
        assert_eq!(restored_cell.radar_left, [31, 32, 33]);
        assert_eq!(
            restored.dynamic_terrain_cells().get(&(0, 0)),
            Some(&DynamicTerrainCellState::capture(&runtime_actual))
        );
    }

    #[test]
    fn save_write_reports_the_failed_stage() {
        let directory = isolated_directory("save-write-stage");
        let repository = SaveRepository::at(&directory);
        let bytes = snapshot("exact description", 123);
        let path = repository.write_named("saved.bin", &bytes).unwrap();
        assert_eq!(repository.read(&path).unwrap(), bytes);

        // A directory at the chosen file path forces the actual write stage to fail.
        std::fs::create_dir(directory.join("blocked.bin")).unwrap();
        let error = repository.write_named("blocked.bin", &bytes).unwrap_err();
        assert_eq!(error.stage(), SaveWriteStage::WriteFile);

        let error = SaveRepository::at(&path)
            .write_named("unreachable.bin", &bytes)
            .unwrap_err();
        assert_eq!(error.stage(), SaveWriteStage::CreateDirectory);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn quickload_selects_newest_modified_time_over_embedded_time() {
        let directory = isolated_directory("save-latest-policies");
        let repository = SaveRepository::at(&directory);
        let embedded_newer_path = repository
            .write_named("embedded-newer.bin", &snapshot("newer", 200))
            .expect("write embedded-newer fixture");
        let embedded_older_path = repository
            .write_named("embedded-older.bin", &snapshot("older", 100))
            .expect("write embedded-older fixture");

        let base_modified = SystemTime::now()
            .checked_sub(std::time::Duration::from_secs(60))
            .expect("fixture modification time");
        std::fs::File::options()
            .write(true)
            .open(&embedded_newer_path)
            .expect("open embedded-newer fixture")
            .set_times(std::fs::FileTimes::new().set_modified(base_modified))
            .expect("set embedded-newer modification time");
        std::fs::File::options()
            .write(true)
            .open(&embedded_older_path)
            .expect("open embedded-older fixture")
            .set_times(
                std::fs::FileTimes::new()
                    .set_modified(base_modified + std::time::Duration::from_secs(30)),
            )
            .expect("set embedded-older modification time");

        assert_eq!(
            repository.quickload_path_by_modified_time(),
            Some(embedded_older_path)
        );

        std::fs::remove_dir_all(directory).expect("remove latest-policy fixture directory");
    }

    #[test]
    fn repository_owns_write_read_browser_scan_and_delete() {
        let directory = isolated_directory("save-repository");
        let repository = SaveRepository::at(&directory);
        let mut simulation = Simulation::new();
        simulation
            .session_mut_for_test()
            .set_map_name_for_test("OFFICIAL.MAP".to_string());
        let bytes = GameSnapshot::save_validated(&simulation, 1, 2, "Northern ridge", 3);

        let path = repository
            .write_named("completely_different_name.bin", &bytes)
            .expect("write repository fixture");
        assert_eq!(
            repository.read(&path).expect("read repository fixture"),
            bytes
        );

        let entries = repository.browser_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0.path, path);
        assert_eq!(entries[0].0.header.description, "Northern ridge");

        repository.delete(&path).expect("delete repository fixture");
        assert!(!repository.exists(&path));
        std::fs::remove_dir_all(directory).expect("remove repository fixture directory");
    }
}

#[cfg(all(test, tests_app))]
#[path = "tests/save_write_tests.rs"]
mod save_write_tests;

#[cfg(all(test, tests_app))]
#[path = "tests/save_browser_tests.rs"]
mod save_browser_tests;

pub(crate) mod keyboard;
