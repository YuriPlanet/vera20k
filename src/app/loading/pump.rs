//! Loading attempt, asset lease, phase progress and presentation handoff.
//!
//! This module sits above simulation and owns loading-screen progress behavior
//! verified from gamemd.exe. It also owns the request/session boundary used by
//! app loop. The prepared map, prefix and startup move together. Offline
//! construction yields between completed native operations, then polls the
//! display jobs; progress order and the consuming installation stay here.
//! Private `render` owns frame drawing; this module retains preparation, cadence,
//! failure and after-present continuation policy. Request/phase regressions live
//! in the private `tests` child.

mod render;
pub(crate) use render::push_progress_meter;

#[cfg(all(test, tests_app))]
mod tests;

#[cfg(all(test, tests_app))]
#[path = "pump/country_loading_tests.rs"]
mod country_loading_tests;

#[cfg(all(test, tests_app))]
#[path = "pump/loading_theme_tests.rs"]
mod loading_theme_tests;

use crate::app::AppState;
use crate::app::loading::campaign_presentation::CampaignLoadingPresentation;
use crate::app::loading::composition::{
    LoadingCompositionSnapshot, LoadingParticipantId, LoadingStartAssignment,
    build_loading_composition, build_random_map_loading_composition, loading_authored_size,
    loading_screen_scale,
};
use crate::app::loading::fresh_scenario::FreshScenarioLoadContextDescriptor;
use crate::app::loading::init::{self, MapLoadInitial, MapLoadResult};
use crate::app::loading::progress_row::LoadingProgressRowSnapshot;
use crate::app::match_bootstrap::{LoadingStartup, PreparedMatchStartup};
use crate::app::match_runtime::sim_host::SimWrite;
use crate::assets::asset_manager::AssetManager;
use crate::assets::pal_file::Color;
use crate::assets::pcx_file::PcxFile;
use crate::map::preview::DecodedPreview;
use crate::render::batch::BatchRenderer;
use crate::render::bit_font::BitFont;
use crate::render::gpu::GpuContext;
use crate::render::loading_screen_chrome::{
    LoadingArtVariant, LoadingScreenAtlas, LoadingScreenCompositionAtlasInput, LoadingScreenWidth,
    MmpbMarkerRemap, PreparedLoadingPreviewRgba, build_loading_screen_atlas_with_composition,
};
use crate::render::shell_surface_present::ShellSurfacePresenter;
use crate::rules::color_scheme::{
    ColorSchemeEntry, hsv_to_rgb, scheme_entry_by_name, scheme_hsv_by_entry,
};
use crate::rules::house_colors::{HouseColorIndex, HouseColorRamps};
use crate::rules::skirmish_colors::SkirmishColorMode;
use crate::sim::scenario_bootstrap::StockOfflinePrefixProjection;
use crate::skirmish_launch::{LaunchCountry, SkirmishLaunchSession};
use crate::ui::game_screen::GameScreen;
use crate::ui::shell::geom::ShellScale;
use std::path::{Path, PathBuf};

const STANDARD_SKIRMISH_PROGRESS_MAX: f64 = 100.0;
const PROGRESS_PERCENT_SCALE: f64 = 0.01;
const PERCENT_DISPLAY_SCALE: f64 = 100.0;
const FTOL_EPSILON: f64 = 0.000_001;
/// Static loading colors used only when rules `[Colors]` are unavailable
/// (missing assets / headless); replaced by `resolve_player_colors` once rules
/// load.
pub(super) const FALLBACK_BACKING_RGB: [f32; 3] = [0.22, 0.22, 0.22];
pub(super) const FALLBACK_PROGRESS_RAMP: [Color; 16] = [Color::rgb(180, 180, 180); 16];
#[derive(Debug, Clone, PartialEq)]
pub struct LoadingProgressState {
    max_value: f64,
    current_value: f64,
}

impl LoadingProgressState {
    pub fn standard_skirmish() -> Self {
        Self {
            max_value: STANDARD_SKIRMISH_PROGRESS_MAX,
            current_value: 0.0,
        }
    }

    pub fn current_value(&self) -> f64 {
        self.current_value
    }

    pub fn current_percent(&self) -> f64 {
        self.current_value / self.max_value * PERCENT_DISPLAY_SCALE
    }

    /// Apply gamemd's loading milestone callback gate plus ProgressClass setter.
    ///
    /// Only strictly advancing callback percentages reach the setter. The setter
    /// stores `max * 0.01 * percent`, clamps only above max, and returns `false`
    /// when the stored value did not change.
    pub fn advance_progress(&mut self, percent: u32) -> bool {
        let requested = f64::from(percent);
        if self.current_percent() >= requested {
            return false;
        }

        self.set_percent(requested)
    }

    pub(super) fn set_percent(&mut self, percent: f64) -> bool {
        let mut new_value = self.max_value * PROGRESS_PERCENT_SCALE * percent;
        if new_value > self.max_value {
            new_value = self.max_value;
        }
        if self.current_value == new_value {
            return false;
        }
        self.current_value = new_value;
        true
    }

    pub fn fill_width_gamemd_ftol_positive_domain(&self, frame0_width: u32) -> u32 {
        // Malformed signed peer progress remains state-visible, but cannot
        // request a negative GPU fill width.
        if self.current_value <= 0.0 {
            return 0;
        }
        gamemd_ftol_positive_domain(f64::from(frame0_width) * self.current_value / self.max_value)
            .max(0) as u32
    }
}

/// Receives a raw native loading milestone at a real load-phase boundary.
///
/// Implementors apply the monotonic gate (via [`LoadingProgressState::advance_progress`])
/// and may synchronously repaint the loading screen on an advancing milestone,
/// mirroring gamemd's per-milestone synchronous hidden-to-primary blit.
/// Selected maps use raw values through 100; random-map seed loads halve raw
/// values and finish at raw 200.
pub(crate) trait LoadingProgressSink {
    fn milestone(&mut self, percent: u32) -> anyhow::Result<()>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NativeLoadingProgressCadence {
    SelectedMap,
    RandomMapHalved,
}

impl NativeLoadingProgressCadence {
    fn for_selected_map_file(selected_map_file: &str) -> Self {
        if crate::map::rmg::is_seed_selection(selected_map_file) {
            Self::RandomMapHalved
        } else {
            Self::SelectedMap
        }
    }

    fn effective_percent(self, raw_percent: u32) -> u32 {
        match self {
            Self::SelectedMap => raw_percent,
            // Native integer division truncates raw random-map milestones.
            Self::RandomMapHalved => raw_percent / 2,
        }
    }

    fn terminal_raw_percent(self) -> u32 {
        match self {
            Self::SelectedMap => 100,
            Self::RandomMapHalved => 200,
        }
    }

    /// Whether native has resolved Scenario inputs before the first loading frame.
    ///
    /// Selected maps also need their parsed preview. Random maps take pixels
    /// from `RandMap.img`; both branches swallow the loader's raw 8 here and
    /// hand it to the first post-frame pump. Native generates a random map
    /// after its first loading frame (Full_Init inside Generate, the
    /// generation reported as 50–99%), which VERA20k does before it — a
    /// recorded residual of the random-map chain.
    fn prepares_scenario_before_first_frame(self) -> bool {
        match self {
            Self::SelectedMap | Self::RandomMapHalved => true,
        }
    }
}

/// A sink that only advances the gated progress state, with no repaint. Used at
/// the pump call sites before the render-triggering sink is constructed, and as
/// the base behavior shared by all sinks.
struct GatedProgressSink<'a> {
    progress: &'a mut LoadingProgressState,
    progress_row: &'a mut LoadingProgressRowSnapshot,
    cadence: NativeLoadingProgressCadence,
    network: Option<crate::app::lan_service::LoadingNetworkService<'a>>,
}

impl GatedProgressSink<'_> {
    fn advance(&mut self, raw_percent: u32) -> bool {
        self.progress
            .advance_progress(self.cadence.effective_percent(raw_percent))
    }

    fn service(&mut self) -> anyhow::Result<()> {
        if let Some(network) = self.network.as_mut() {
            network.milestone(
                gamemd_ftol_positive_domain(self.progress.current_percent()),
                self.progress_row,
            )?;
        }
        Ok(())
    }
}

impl LoadingProgressSink for GatedProgressSink<'_> {
    fn milestone(&mut self, raw_percent: u32) -> anyhow::Result<()> {
        self.advance(raw_percent);
        //69AEF1 skips only the local redraw for nonadvancing raw values;
        //the same callback still reports displayed progress and services I/O.
        self.service()
    }
}

/// Sink for the generic (non-native) map load, which has no progress bar.
struct NoopProgressSink;

impl LoadingProgressSink for NoopProgressSink {
    fn milestone(&mut self, _percent: u32) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Select progress policy and its loader metadata once. The phase consumes the
/// same startup/resources regardless of whether native loading art is available.
struct LoadingPhaseProgress<'a> {
    sink: SelectedProgressSink<'a>,
    native_theater_cache_mismatch: bool,
    runtime_color_scheme_count: usize,
}

enum SelectedProgressSink<'a> {
    Rendering(RenderingProgressSink<'a>),
    Gated(GatedProgressSink<'a>),
    Generic(NoopProgressSink),
}

impl LoadingProgressSink for SelectedProgressSink<'_> {
    fn milestone(&mut self, raw_percent: u32) -> anyhow::Result<()> {
        match self {
            Self::Rendering(sink) => sink.milestone(raw_percent),
            Self::Gated(sink) => sink.milestone(raw_percent),
            Self::Generic(sink) => sink.milestone(raw_percent),
        }
    }
}

impl<'a> LoadingPhaseProgress<'a> {
    fn select(
        native: Option<&'a mut NativeLoadingScreenState>,
        mismatch: bool,
        show_progress: bool,
        network: Option<crate::app::lan_service::LoadingNetworkService<'a>>,
        rendering: impl FnOnce(
            &'a mut NativeLoadingScreenState,
            Option<crate::app::lan_service::LoadingNetworkService<'a>>,
        ) -> RenderingProgressSink<'a>,
    ) -> Self {
        let Some(native) = native else {
            return Self {
                sink: SelectedProgressSink::Generic(NoopProgressSink),
                native_theater_cache_mismatch: false,
                runtime_color_scheme_count: 0,
            };
        };
        let runtime_color_scheme_count = native.runtime_color_scheme_count;
        let sink = if show_progress && native.atlas.is_some() {
            SelectedProgressSink::Rendering(rendering(native, network))
        } else {
            SelectedProgressSink::Gated(GatedProgressSink {
                cadence: native.progress_cadence,
                progress: &mut native.progress,
                progress_row: &mut native.progress_row,
                network,
            })
        };
        Self {
            sink,
            native_theater_cache_mismatch: mismatch,
            runtime_color_scheme_count,
        }
    }
}

pub(crate) struct LoadingRequest {
    startup: LoadingStartup,
    /// Accepted setup start staging is small, provenance-bearing gameplay
    /// input. It is never reconstructed from the presentation preview.
    accepted_rmg_start_staging: Option<crate::app::shell_random_map::AcceptedRmgStartStaging>,
    /// Setup-generated preview retained only as a loading-composition fallback.
    /// It never supplies gameplay map data, RNG continuation, or constructors.
    random_map_preview: Option<crate::map::rmg::GeneratedMap>,
    /// Phobos `[General] SkirmishUnlimitedColors=` for this load. The match
    /// owner stamps it from the rules it just loaded; the constructors leave it
    /// unset, which resolves to the vanilla priority table.
    skirmish_colors: Option<SkirmishColorMode>,
    /// Freeze the process flag before the early generated-map Full_Init; the
    /// later continuation consumes its result without selecting sources again.
    the_team_enabled: bool,
    /// Replay frame zero or process staging from an earlier network game.
    supplied_tiberium: Option<crate::sim::scenario_session::TiberiumFlags>,
}

impl LoadingRequest {
    pub(crate) fn replay(
        startup: &crate::sim::replay::ReplayStartup,
        map_beside: Option<&str>,
    ) -> anyhow::Result<Self> {
        let map_file = map_beside.unwrap_or(startup.map_name());
        let seed = crate::app::match_bootstrap::MatchSeed {
            value: startup.seed(),
            source: crate::app::match_bootstrap::MatchSeedSource::SavedScenario,
            seed_authority_certifying: false,
        };
        let game_mode = startup.game_mode();
        let native_id_prefix = startup.native_id_prefix()?;
        let loading_startup = if let Some(start) = startup.authored_campaign() {
            LoadingStartup::AuthoredCampaignReplay {
                start: Box::new(start.clone()),
                logical_map: startup.map_name().to_owned(),
                local_map: map_file.to_owned(),
                map_sha256: startup.map_sha256().to_owned(),
                game_speed: startup.game_speed(),
                seed,
                game_mode,
                native_id_prefix,
            }
        } else {
            LoadingStartup::Replay {
                launch: Box::new(startup.launch_descriptor(map_file)?),
                game_mode,
                native_id_prefix,
                seed,
            }
        };
        Ok(Self {
            startup: loading_startup,
            accepted_rmg_start_staging: None,
            random_map_preview: None,
            skirmish_colors: None,
            the_team_enabled: false,
            supplied_tiberium: Some(startup.tiberium_flags()),
        })
    }

    pub(crate) fn campaign(startup: crate::app::match_bootstrap::PreparedCampaignStartup) -> Self {
        Self {
            startup: LoadingStartup::Campaign(startup),
            accepted_rmg_start_staging: None,
            random_map_preview: None,
            skirmish_colors: None,
            the_team_enabled: false,
            supplied_tiberium: None,
        }
    }

    pub(crate) fn network(startup: LoadingStartup) -> Self {
        assert!(
            matches!(startup, LoadingStartup::Network(_)),
            "network request requires an admitted immutable start"
        );
        Self {
            startup,
            accepted_rmg_start_staging: None,
            random_map_preview: None,
            skirmish_colors: None,
            the_team_enabled: false,
            supplied_tiberium: None,
        }
    }

    pub(crate) fn accepted_skirmish(startup: PreparedMatchStartup) -> Self {
        Self {
            startup: LoadingStartup::Accepted(startup),
            accepted_rmg_start_staging: None,
            random_map_preview: None,
            skirmish_colors: None,
            the_team_enabled: false,
            supplied_tiberium: None,
        }
    }

    pub(crate) fn unverified_legacy_skirmish(
        skirmish_launch_session: SkirmishLaunchSession,
        seed: crate::app::match_bootstrap::MatchSeed,
    ) -> Self {
        Self {
            startup: LoadingStartup::UnverifiedLegacy {
                session: skirmish_launch_session,
                seed,
            },
            accepted_rmg_start_staging: None,
            random_map_preview: None,
            skirmish_colors: None,
            the_team_enabled: false,
            supplied_tiberium: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn generic_map_load(selected_map_file: impl Into<String>) -> Self {
        Self {
            startup: LoadingStartup::Generic {
                selected_map_file: selected_map_file.into(),
            },
            accepted_rmg_start_staging: None,
            random_map_preview: None,
            skirmish_colors: None,
            the_team_enabled: false,
            supplied_tiberium: None,
        }
    }

    pub(crate) fn selected_map_file(&self) -> &str {
        self.startup().selected_map_file()
    }

    /// The player-colour conversion this load resolves priorities with.
    pub(crate) fn skirmish_colors(&self) -> Option<SkirmishColorMode> {
        self.skirmish_colors
    }

    /// Stamp the loaded rules' Phobos `SkirmishUnlimitedColors` setting. Only
    /// the match owner that just loaded rules calls this; the constructors
    /// above leave the request on the vanilla conversion.
    pub(crate) fn with_skirmish_colors(mut self, mode: Option<SkirmishColorMode>) -> Self {
        self.skirmish_colors = mode;
        self
    }

    /// Run the request's authoritative initial-map entry. The retained random
    /// preview is intentionally not an argument: active retail's `.SED` reader
    /// regenerates gameplay after Start, while the preview remains a loading-
    /// composition fallback only. VERA: after a network room's checks of the
    /// map file, the map takes the mod's map additions
    /// (`MapLoadInitial::with_mod_additions`).
    fn load_initial_with_assets(
        &self,
        ra2_dir: std::path::PathBuf,
        asset_manager: &mut AssetManager,
        native_rules: Option<&mut crate::rules::process_owner::NativeRulesProcessOwner>,
        resident: Option<&mut crate::sim::world::Simulation>,
        map_additions: Option<&crate::app::mod_additions::MapAdditions>,
        progress: &mut dyn LoadingProgressSink,
    ) -> anyhow::Result<MapLoadInitial> {
        let replay_map = match self.startup() {
            LoadingStartup::AuthoredCampaignReplay {
                logical_map,
                local_map,
                map_sha256,
                ..
            } if local_map == logical_map => crate::map::source::matching_loose_map_path(
                &ra2_dir,
                asset_manager.mod_dir(),
                logical_map,
                map_sha256,
            ),
            _ => None,
        };
        let map_file = replay_map
            .as_deref()
            .and_then(Path::to_str)
            .unwrap_or_else(|| self.startup().physical_map_file());
        let mut loaded = init::load_map_initial_with_assets(
            ra2_dir,
            asset_manager,
            native_rules,
            resident,
            Some(map_file),
            self.startup()
                .launch_session()
                .map(|launch| launch.mode.override_file.as_str()),
            self.startup().session_mode(),
            self.startup().ai_file(asset_manager),
            self.the_team_enabled,
            self.startup().match_seed(),
            progress,
        )?;
        if let LoadingStartup::Network(_) = self.startup() {
            loaded.bind_network_logical_seed_name(self.selected_map_file())?;
        }
        if let LoadingStartup::Network(startup) = self.startup()
            && let Some(expected) = startup.local_map_source()
        {
            anyhow::ensure!(
                loaded.map_source().physical_identity() == expected.physical_identity(),
                "network map bytes changed after admission"
            );
            if !matches!(
                loaded.map_source(),
                crate::map::source::LoadedMapSource::Generated { .. }
            ) {
                anyhow::ensure!(
                    startup.effective_map_digest()
                        == Some(
                            crate::map::source::effective_network_map_digest(
                                &loaded.map_data().ini,
                                false,
                                false
                            )
                            .as_str()
                        ),
                    "network map include files changed after admission"
                );
            }
        }
        // The client applies the code for its mode to the map it launches.
        // The native mode object and the normal Rules layers still own their
        // separate setup. Only the imported catalog's own maps take this code.
        if self.startup().session_mode()
            == crate::app::match_runtime::session::SessionMode::Skirmish
            && let Some(session) = self.skirmish_launch_session()
            && let Some(additions) = crate::app::mod_additions::MapAdditions::client_mode(
                asset_manager,
                self.selected_map_file(),
                &session.mode,
            )
            .map_err(anyhow::Error::msg)?
        {
            loaded = loaded.with_mod_additions(&additions)?;
        }
        match map_additions {
            Some(additions) => loaded.with_mod_additions(additions),
            None => Ok(loaded),
        }
    }

    fn skirmish_launch_session(&self) -> Option<&SkirmishLaunchSession> {
        self.startup().launch_session()
    }

    fn startup(&self) -> &LoadingStartup {
        &self.startup
    }

    /// Attach the setup preview for presentation fallback only. Scenario read
    /// 0x00684620 regenerates the accepted `.SED`; that launch result, not this
    /// preview, resolves the stock Scenario prefix and initializes gameplay.
    #[cfg(test)]
    pub(crate) fn with_random_map_preview(
        mut self,
        random_map_preview: Option<crate::map::rmg::GeneratedMap>,
    ) -> Self {
        self.random_map_preview = random_map_preview;
        self
    }

    /// Transfer one accepted setup transaction into loading. Preview terrain,
    /// MapGen continuation, and construction trace remain presentation-only;
    /// the separately extracted staging becomes the active Scenario waypoint
    /// authority for Gather, loading markers, and the live session snapshot.
    pub(crate) fn with_accepted_random_map(
        mut self,
        accepted: Option<crate::app::shell_random_map::AcceptedRandomMapLaunch>,
    ) -> Self {
        if let Some(accepted) = accepted {
            let (preview, start_staging) = accepted.into_parts();
            self.random_map_preview = Some(preview);
            self.accepted_rmg_start_staging = Some(start_staging);
        }
        self
    }

    /// A guest's random-map start staging from the host's `'W'`; the guest
    /// has no setup preview of its own.
    pub(crate) fn with_start_staging(
        mut self,
        staging: Option<crate::app::shell_random_map::AcceptedRmgStartStaging>,
    ) -> Self {
        if staging.is_some() {
            self.accepted_rmg_start_staging = staging;
        }
        self
    }

    #[cfg(test)]
    fn random_map_preview(&self) -> Option<&crate::map::rmg::GeneratedMap> {
        self.random_map_preview.as_ref()
    }

    fn admit_context(
        &mut self,
        initial: &MapLoadInitial,
    ) -> anyhow::Result<FreshScenarioLoadContextDescriptor> {
        FreshScenarioLoadContextDescriptor::admit(
            &self.startup,
            initial.map_data(),
            initial.map_source(),
            &mut self.accepted_rmg_start_staging,
        )
    }

    fn prepare(
        self,
        ra2_dir: PathBuf,
        assets: &mut AssetManager,
        native_rules: Option<&mut crate::rules::process_owner::NativeRulesProcessOwner>,
        map_additions: Option<&crate::app::mod_additions::MapAdditions>,
        mapgen_retention: &mut crate::app::shell_random_map::RandomMapGenerationRetention,
        mut resident: Option<&mut crate::sim::world::Simulation>,
        progress: &mut dyn LoadingProgressSink,
    ) -> anyhow::Result<PreparedScenarioLoad> {
        let mut initial = self.load_initial_with_assets(
            ra2_dir,
            assets,
            native_rules,
            resident.as_deref_mut(),
            map_additions,
            progress,
        )?;
        initial.retain_process_state(
            mapgen_retention,
            resident,
            self.startup().match_spec().topology(),
        );
        self.prepare_initial(initial)
    }

    #[cfg(test)]
    pub(crate) fn load_random_map_snapshot_for_test(
        self,
        ra2_dir: PathBuf,
        assets: &mut AssetManager,
        native_rules: Option<&mut crate::rules::process_owner::NativeRulesProcessOwner>,
        mapgen_retention: &mut crate::app::shell_random_map::RandomMapGenerationRetention,
        progress: &mut dyn LoadingProgressSink,
    ) -> anyhow::Result<init::RandomMapLaunchSnapshot> {
        let prepared = self.prepare(
            ra2_dir,
            assets,
            native_rules,
            None,
            mapgen_retention,
            None,
            progress,
        )?;
        Ok(prepared
            .initial
            .into_random_map_launch_snapshot(assets, prepared.context))
    }

    #[cfg(test)]
    pub(crate) fn prepare_random_map_snapshot_for_test(
        self,
        initial: MapLoadInitial,
        assets: &mut AssetManager,
    ) -> anyhow::Result<init::RandomMapLaunchSnapshot> {
        let prepared = self.prepare_initial(initial)?;
        Ok(prepared
            .initial
            .into_random_map_launch_snapshot(assets, prepared.context))
    }

    fn prepare_initial(
        mut self,
        mut initial: MapLoadInitial,
    ) -> anyhow::Result<PreparedScenarioLoad> {
        let context = self.admit_context(&initial)?;
        initial.supplied_tiberium = self.supplied_tiberium;
        Ok(PreparedScenarioLoad {
            request: self,
            initial,
            context,
            rules_prefix: None,
        })
    }
}

/// VERA-internal ownership protocol, gamemd equivalent UNCHECKED. Native phase
/// order is independently established in
/// LOADING_FIRST_RENDERER_CORRECTED_COMPOSITION_DATA_READINESS_GHIDRA_REPORT.md
/// (Full_Init00687558..00687594). The parsed map, admitted prefix and startup
/// travel together from preparation through composition to remaining loading.
struct PreparedScenarioLoad {
    request: LoadingRequest,
    initial: MapLoadInitial,
    context: FreshScenarioLoadContextDescriptor,
    /// Bound only when the Rules are read the Ares way; `None` keeps the stock prefix.
    rules_prefix: Option<crate::rules::process_owner::NativeNonCampaignPrefixPreparation>,
}

struct ConstructingScenarioLoad {
    load: init::FreshMapLoadJob,
    /// Bound only when the Rules are read the Ares way; `None` keeps the stock prefix.
    rules_prefix: Option<crate::rules::process_owner::NativeNonCampaignPrefixPreparation>,
}

fn retire_rules_prefix(
    preparation: Option<crate::rules::process_owner::NativeNonCampaignPrefixPreparation>,
    assets: &mut crate::app::process_assets::ProcessAssets,
) {
    if let Some(preparation) = preparation {
        assets
            .native_rules_mut_with_tile_cache()
            .0
            .expect("prepared prefix retains its process owner")
            .cancel_prepared_noncampaign_scenario_prefix(preparation);
    }
}

impl PreparedScenarioLoad {
    fn cancel_rules_prefix(&mut self, assets: &mut crate::app::process_assets::ProcessAssets) {
        retire_rules_prefix(self.rules_prefix.take(), assets);
    }
}

#[expect(
    clippy::large_enum_variant,
    reason = "one stage per loading session; Prepared carries the Selected request onward"
)]
enum LoadingStage {
    Selected(LoadingRequest),
    Prepared(PreparedScenarioLoad),
    ConstructingScenario(ConstructingScenarioLoad),
    PreparingPresentation {
        result: Box<MapLoadResult>,
    },
    AwaitingPeers {
        result: Box<MapLoadResult>,
        wait: NetworkLoadWait,
    },
    /// Complete has been decided and terminal100 submitted to the same reliable
    /// owner; that is not an ACK. The loaded-screen hold keeps this result here.
    ReadyToInstall {
        result: Box<MapLoadResult>,
    },
}

impl LoadingStage {
    fn startup(&self) -> &LoadingStartup {
        match self {
            Self::Selected(request) => request.startup(),
            Self::Prepared(prepared) => prepared.request.startup(),
            Self::ConstructingScenario(constructing) => constructing.load.startup(),
            Self::PreparingPresentation { result }
            | Self::AwaitingPeers { result, .. }
            | Self::ReadyToInstall { result } => &result.scenario.startup,
        }
    }
}

impl LoadingSession {
    #[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
    fn await_browser_network_peers(
        result: Box<MapLoadResult>,
        native: Option<NativeLoadingScreenState>,
        job: LoadingJob,
    ) -> Self {
        let progress = native.as_ref().expect("network load retains progress rows");
        let now = (crate::app::match_runtime::frame_pacer::wall_clock_ms() >> 4) as i32;
        let mut wait = NetworkLoadWait::new(
            now,
            progress.progress_row.total_fraction(&progress.progress),
        );
        // The browser still owes the terminal milestone's ACK drain. Anchor
        // the peer wait only when that drain has completed on a later turn.
        wait.start_pending = true;
        Self {
            stage: LoadingStage::AwaitingPeers { result, wait },
            native,
            job,
            next_frame: NextLoadingFrame::Later,
            install_after_ms: None,
        }
    }
}

/// Original684370. Wall-clock buckets use the existing signed countdown;
/// this wait owns no roster or progress copy and never advances simulation.
struct NetworkLoadWait {
    timeout: crate::sim::timer::CdTimer,
    resend: crate::sim::timer::CdTimer,
    last_fraction: f64,
    #[cfg(any(
        test,
        all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown")
    ))]
    start_pending: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NetworkLoadPoll {
    Pending,
    ReportProgress,
    Complete,
    TimedOut,
}

impl NetworkLoadWait {
    fn new(now: i32, fraction: f64) -> Self {
        Self {
            timeout: crate::sim::timer::CdTimer::started(now, 3600),
            resend: crate::sim::timer::CdTimer::started(now, 300),
            last_fraction: fraction,
            #[cfg(any(
                test,
                all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown")
            ))]
            start_pending: false,
        }
    }
    fn poll(&mut self, now: i32, fraction: f64) -> NetworkLoadPoll {
        #[cfg(any(
            test,
            all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown")
        ))]
        if self.start_pending {
            *self = Self::new(now, fraction);
        }
        // Expiry precedes the completion check at68442D..684465.
        if self.timeout.expired(now) {
            return NetworkLoadPoll::TimedOut;
        }
        if fraction >= 0.9995 {
            return NetworkLoadPoll::Complete;
        }
        if fraction != self.last_fraction {
            self.timeout.start(now, 7200);
            self.last_fraction = fraction;
        }
        if self.resend.expired(now) {
            self.resend.start(now, 60);
            NetworkLoadPoll::ReportProgress
        } else {
            NetworkLoadPoll::Pending
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeLoadingArt {
    StockOffline(Option<LoadingArtVariant>),
    Campaign,
}

pub(crate) struct NativeLoadingScreenState {
    pub variant: NativeLoadingArt,
    local_side_index: i32,
    /// Both bound only when the Rules are read the Ares way; `None` keeps the stock screen.
    early_country_loading: Option<EarlyCountryLoading>,
    late_country_loading: Option<crate::rules::country_loading::AresCountryLoading>,
    /// Local player's MP color scheme — source of the progress outline and bar
    /// remap. Derived from the launch session, not the country variant.
    pub color_index: HouseColorIndex,
    /// Player `[Colors]` base color (normalized RGB) from that scheme's HSV, resolved
    /// through native `HSV_To_RGB`. Draws the meter's `+0x71` outline and the row
    /// label. Falls back to a static shade when `[Colors]` is unavailable.
    pub backing_rgb: [f32; 3],
    /// GAME.FNT loading-copy color from the named AlliedLoad/SovietLoad scheme.
    pub text_rgb: [f32; 3],
    /// Full `[Colors]` band copied into PROGBARM palette indices 16..31 when
    /// the session-local loading atlas is decoded.
    pub progress_ramp: [Color; 16],
    pub progress: LoadingProgressState,
    pub progress_row: LoadingProgressRowSnapshot,
    pub atlas: Option<LoadingScreenAtlas>,
    pub composition: Option<LoadingCompositionSnapshot>,
    pub campaign_presentation: Option<super::campaign_presentation::CampaignLoadingPresentation>,
    campaign_names: Option<crate::rules::campaign_loading::CampaignScenarioNames>,
    /// Native constructs two runtime ColorScheme objects per current `[Colors]`
    /// entry. Capture that pre-load count before the later rules reset.
    runtime_color_scheme_count: usize,
    progress_cadence: NativeLoadingProgressCadence,
}

/// Two immutable observations, not a second Country owner: Color/PAL are
/// resolved before Full_Init; the later file/labels are read after E_multi.
struct EarlyCountryLoading {
    country: crate::rules::ruleset::CountryIdx,
    palette_name: String,
    palette: Option<crate::assets::pal_file::Palette>,
    /// `None` keeps the stock AlliedLoad/SovietLoad text color.
    text_rgb: Option<[f32; 3]>,
}

impl NativeLoadingScreenState {
    fn standard_skirmish(
        variant: Option<LoadingArtVariant>,
        local_side_index: i32,
        color_index: HouseColorIndex,
        progress_row: LoadingProgressRowSnapshot,
        progress_cadence: NativeLoadingProgressCadence,
    ) -> Self {
        Self {
            variant: NativeLoadingArt::StockOffline(variant),
            local_side_index,
            early_country_loading: None,
            late_country_loading: None,
            color_index,
            // Static placeholders; replaced by `resolve_player_colors` once rules load.
            backing_rgb: FALLBACK_BACKING_RGB,
            text_rgb: [1.0, 1.0, 1.0],
            progress_ramp: FALLBACK_PROGRESS_RAMP,
            progress: LoadingProgressState::standard_skirmish(),
            progress_row,
            atlas: None,
            composition: None,
            campaign_presentation: None,
            campaign_names: None,
            runtime_color_scheme_count: 0,
            progress_cadence,
        }
    }

    fn campaign(cd: i32) -> Self {
        Self {
            variant: NativeLoadingArt::Campaign,
            // BeginProgress684620 passes Campaign.CD; color resolver642B30
            // distinguishes exactly zero from all nonzero values.
            local_side_index: cd,
            early_country_loading: None,
            late_country_loading: None,
            color_index: HouseColorIndex(0),
            backing_rgb: FALLBACK_BACKING_RGB,
            text_rgb: [1.0; 3],
            progress_ramp: FALLBACK_PROGRESS_RAMP,
            progress: LoadingProgressState::standard_skirmish(),
            // Campaign meter has no participant label, remap or country icon.
            progress_row: LoadingProgressRowSnapshot::default(),
            atlas: None,
            composition: None,
            campaign_presentation: None,
            campaign_names: None,
            runtime_color_scheme_count: 0,
            progress_cadence: NativeLoadingProgressCadence::SelectedMap,
        }
    }

    fn suppresses_chrome(&self) -> bool {
        self.campaign_presentation
            .as_ref()
            .is_some_and(|presentation| presentation.background_name.is_empty())
    }

    /// Resolve the backing fill + progress ramp from the rules `[Colors]` data. The
    /// player's `color_index` is a `[Colors]` entry index: the backing is that
    /// entry's HSV→RGB, and PROGBARM uses all 16 shades of that entry's ramp. Leaves the static
    /// fallbacks in place when the color list is empty/unmatched.
    fn resolve_player_colors(&mut self, schemes: &[ColorSchemeEntry], ramps: &HouseColorRamps) {
        self.runtime_color_scheme_count = schemes.len() * 2;
        let entry = self.color_index.0 as usize;
        if let Some(hsv) = scheme_hsv_by_entry(schemes, entry) {
            self.backing_rgb = normalize_rgb(hsv_to_rgb(hsv));
        }
        let text_scheme =
            if self.variant == NativeLoadingArt::StockOffline(Some(LoadingArtVariant::Observer)) {
                "LightGrey"
            } else if self.local_side_index == 0 {
                "AlliedLoad"
            } else {
                "SovietLoad"
            };
        if let Some(entry) = scheme_entry_by_name(schemes, text_scheme)
            && let Some(hsv) = scheme_hsv_by_entry(schemes, entry)
        {
            self.text_rgb = normalize_rgb(hsv_to_rgb(hsv));
        } else {
            log::warn!("Missing loading text color scheme {text_scheme}; using white");
        }
        // Ares1002F8F0 publishes this signed index once for the loading
        // attempt; negative resumes the stock side-color branch above.
        // GetSideColor642B60's hook (0x642B91, 1002F870) follows its
        // observer LightGrey test, and an observer publishes no Country here.
        if let Some(text_rgb) = self
            .early_country_loading
            .as_ref()
            .and_then(|early| early.text_rgb)
        {
            self.text_rgb = text_rgb;
        }
        self.progress_ramp = *ramps.ramp(self.color_index);
        for row in &mut self.progress_row.participants {
            if let Some(hsv) = scheme_hsv_by_entry(schemes, usize::from(row.color_index.0)) {
                row.backing_rgb = normalize_rgb(hsv_to_rgb(hsv));
            }
            row.ramp = *ramps.ramp(row.color_index);
        }
    }
}

pub(crate) struct LoadingSession {
    stage: LoadingStage,
    native: Option<NativeLoadingScreenState>,
    job: LoadingJob,
    /// The loading frame the next present draws. Native sessions start with
    /// the black frame that replaces the closed shell: Main_Game hides the
    /// cursor, fills the hidden surface black and blits it
    /// (`0x0052E64C..0x0052E69E`) before Start_Scenario reads the map, and
    /// nothing else reaches the screen until the loading screen's first
    /// repaint. Generic sessions start at their first frame.
    next_frame: NextLoadingFrame,
    /// VERA-only: once every player has loaded, a network match keeps its
    /// loading screen up until this wall-clock time
    /// ([`NETWORK_LOADED_HOLD_MS`]) before installing the scenario.
    install_after_ms: Option<u64>,
}

/// How long a network match's loading screen stays up after every player
/// has loaded. Presentation only: the match starts from the same state.
const NETWORK_LOADED_HOLD_MS: u64 = 2_000;

impl LoadingSession {
    /// Whether the loaded screen still holds at `now_ms`.
    fn holds_loaded_screen(&self, now_ms: u64) -> bool {
        self.install_after_ms.is_some_and(|after| now_ms < after)
    }

    fn retire(mut self, assets: &mut crate::app::process_assets::ProcessAssets) {
        if let LoadingStage::Prepared(prepared) = &mut self.stage {
            prepared.cancel_rules_prefix(assets);
        }
        if let LoadingStage::ConstructingScenario(constructing) = &mut self.stage {
            retire_rules_prefix(constructing.rules_prefix.take(), assets);
        }
        if let LoadingStage::PreparingPresentation { result }
        | LoadingStage::AwaitingPeers { result, .. }
        | LoadingStage::ReadyToInstall { result } = &mut self.stage
            && let Some(manager) = result.asset_manager.take()
        {
            assets.return_from_loading(manager);
        }
        self.job.retire(assets);
    }

    fn failure_policy(&self) -> LoadingFailurePolicy {
        if self.stage.startup().campaign().is_some() {
            LoadingFailurePolicy::RetryCampaignShell
        } else if matches!(self.stage.startup(), LoadingStartup::Network(_)) {
            LoadingFailurePolicy::RetryNetworkShell
        } else if self.native.is_some() {
            LoadingFailurePolicy::ReportNativeFailure
        } else {
            LoadingFailurePolicy::InstallGenericFallback
        }
    }

    fn native_pump_blocked(&self) -> bool {
        self.native
            .as_ref()
            .is_some_and(|native| native.atlas.is_none() && !native.suppresses_chrome())
    }

    fn from_request(request: LoadingRequest) -> Self {
        // Phobos SkirmishUnlimitedColors is a rules-owned conversion the caller
        // stamps on the request; a test or a load without rules keeps vanilla.
        let skirmish_colors = request.skirmish_colors().unwrap_or_default();
        let native = match request.skirmish_launch_session() {
            Some(skirmish_launch_session) => {
                let variant = if skirmish_launch_session.local_player().is_observer() {
                    Some(LoadingArtVariant::Observer)
                } else {
                    loading_art_variant_from_launch_country(
                        skirmish_launch_session.local_slot().country,
                    )
                };
                // `local.color_index` is the gamemd color priority; resolve it
                // to a `[Colors]` entry index through the active mode.
                let color_index = HouseColorIndex(
                    skirmish_colors
                        .entry_for_priority(skirmish_launch_session.local_slot().color_index as i32)
                        as u8,
                );
                let progress_cadence = NativeLoadingProgressCadence::for_selected_map_file(
                    request.selected_map_file(),
                );
                Some(NativeLoadingScreenState::standard_skirmish(
                    variant,
                    -1, // Bound from the rules-owned HouseType Side before drawing.
                    color_index,
                    LoadingProgressRowSnapshot::from_launch_session(
                        skirmish_launch_session,
                        skirmish_colors,
                    ),
                    progress_cadence,
                ))
            }
            None => request
                .startup()
                .campaign()
                .map(|startup| NativeLoadingScreenState::campaign(startup.campaign().cd())),
        };
        Self {
            stage: LoadingStage::Selected(request),
            next_frame: if native.is_some() {
                NextLoadingFrame::Blank
            } else {
                NextLoadingFrame::First
            },
            native,
            job: LoadingJob::new(),
            install_after_ms: None,
        }
    }
}

enum LoadingPump {
    Pending,
    Finished {
        result: Box<MapLoadResult>,
        timed_out: Vec<usize>,
    },
    Failed(anyhow::Error),
}

pub(crate) enum LoadingRenderResult {
    NativeRendered,
    GenericFallback,
    Failed,
}

struct LoadingJob {
    ra2_dir: Option<PathBuf>,
    asset_manager: Option<AssetManager>,
}

impl LoadingJob {
    fn new() -> Self {
        Self {
            ra2_dir: None,
            asset_manager: None,
        }
    }
}

fn ordinary_country_loading_index(
    startup: &LoadingStartup,
) -> Option<crate::rules::ruleset::CountryIdx> {
    // A replay of a network game has the same cold Rules boundary as a live
    // match. An ordinary Country prefix would transfer the old world's writes
    // into that rebuilt registry after the initial map load.
    if startup.match_spec().topology().is_lockstep()
        || matches!(
            startup,
            LoadingStartup::Campaign(_) | LoadingStartup::Generic { .. }
        )
        || crate::map::rmg::is_seed_selection(startup.selected_map_file())
    {
        return None;
    }
    let launch = startup.launch_session()?;
    if launch.local_player().is_observer() {
        return None;
    }
    Some(launch.local_slot().country.index())
}

fn bind_early_country_loading(
    native: &mut NativeLoadingScreenState,
    startup: &LoadingStartup,
    owner: &crate::rules::process_owner::NativeRulesProcessOwner,
    assets: &AssetManager,
) -> anyhow::Result<()> {
    let Some(index) = ordinary_country_loading_index(startup) else {
        return Ok(());
    };
    let (country, side) = owner.country_loading(index).ok_or_else(|| {
        anyhow::anyhow!(
            "loading Country {} was not allocated by session admission",
            index.0
        )
    })?;
    // `-ra2`: original game.exe binds one MPLS.PAL for every Country (only
    // the observer uses MPLSOBS.PAL). The YR per-country MPLSU/MPLSR/... PALs
    // in the Country table are absent from an RA2 install, so keep them out
    // of the frozen early observation and let the SHP render with MPLS.PAL.
    let palette_name = if crate::util::ra2_launch::enabled() {
        "MPLS.PAL".to_owned()
    } else {
        country.files().1.to_owned()
    };
    let palette = if palette_name.is_empty() {
        None
    } else {
        // Original 72ADE0 -> 72AE06/72AEDD leaves a missing Convert null;
        // DrawLoading 5535C1/5535C7 then skips only the background. Freeze
        // that absent observation too; a later mount must not reread the PAL.
        crate::render::loading_screen_chrome::load_named_ui_palette(assets, &palette_name)
    };
    let text_rgb = owner
        .country_loading_text_hsv(index)
        .map(|hsv| normalize_rgb(hsv_to_rgb(hsv)));
    native.local_side_index = side;
    if let Some(text_rgb) = text_rgb {
        native.text_rgb = text_rgb;
    }
    native.early_country_loading = Some(EarlyCountryLoading {
        country: index,
        palette_name,
        palette,
        text_rgb,
    });
    Ok(())
}

fn prepare_country_loading_prefix(
    process_assets: &mut crate::app::process_assets::ProcessAssets,
    resident: Option<&mut crate::sim::world::Simulation>,
    prepared: &mut PreparedScenarioLoad,
    native: &mut NativeLoadingScreenState,
) -> anyhow::Result<()> {
    let Some(early) = native.early_country_loading.as_ref() else {
        return Ok(());
    };
    anyhow::ensure!(
        !matches!(
            prepared.initial.map_source(),
            crate::map::source::LoadedMapSource::Generated { .. }
        ),
        "ordinary Country loading prefix cannot consume a generated-map admission"
    );
    let owner = process_assets
        .native_rules_mut_with_tile_cache()
        .0
        .expect("early Country loading retains its process owner");
    if let Some(resident) = resident {
        // This runtime is now permanently outgoing, even if later art or map
        // loading fails. Shell/failure screens retain it only for projection;
        // it must never restart after its runtime Rules writes are transferred.
        resident.retire_scenario_started();
        crate::app::process_assets::transfer_outgoing_simulation_rules(owner, resident)?;
    }
    let preparation = owner.prepare_noncampaign_scenario_prefix();
    native.late_country_loading = Some(
        owner
            .country_loading(early.country)
            .expect("E_multi preserves an already allocated Country")
            .0
            .clone(),
    );
    prepared.rules_prefix = Some(preparation);
    Ok(())
}

/// Admit a peer's loading milestone into the active immutable source roster.
/// Transport is outside this owner; controlled service input uses this same seam.
pub(crate) fn accept_network_loading_progress(
    frontend: &mut crate::app::frontend::state::FrontendState,
    session: &crate::app::match_runtime::session::MatchSession,
    source_index: usize,
    percent: i32,
) -> bool {
    frontend
        .loading_session
        .as_mut()
        .and_then(|loading| loading.native.as_mut())
        .is_some_and(|native| {
            accept_network_loading_progress_row(
                session,
                &mut native.progress_row,
                source_index,
                percent,
            )
        })
}

/// One source-membership and row mutation owner used while LoadingSession is
/// either installed in Frontend or borrowed by its synchronous phase sink.
pub(crate) fn accept_network_loading_progress_row(
    session: &crate::app::match_runtime::session::MatchSession,
    rows: &mut LoadingProgressRowSnapshot,
    source_index: usize,
    percent: i32,
) -> bool {
    session.network_return_context().is_some()
        && session
            .network_connected_source_indices()
            .contains(&source_index)
        && rows.update_remote_progress(source_index, percent)
}

/// True only for the actual active frozen loader, never an old retained world.
pub(crate) fn network_loading_active(
    frontend: &crate::app::frontend::state::FrontendState,
) -> bool {
    matches!(frontend.screen, GameScreen::Loading)
        && frontend
            .loading_session
            .as_ref()
            .is_some_and(|loading| matches!(loading.stage.startup(), LoadingStartup::Network(_)))
}

/// UDP Departure or a definitive HTTPS start/channel/session rejection uses the
/// same failed-load retirement, asset return and shell reentry as loader errors.
pub(crate) fn cancel_network_loading(state: &mut AppState, text: &str) -> bool {
    if !network_loading_active(&state.frontend) {
        return false;
    }
    // No box: the lobby line below is this cancellation's message.
    fail_loading_with(
        state,
        LoadingFailurePolicy::RetryNetworkShell,
        anyhow::anyhow!(text.to_owned()),
        false,
    );
    // The failure has reopened the lobby. Native returns without a word;
    // VERA's lobbies say why the load stopped.
    if !state.frontend.native_shell_ui && !state.frontend.external_launch {
        crate::app::shell_lan::append_chat(
            &state.renderer,
            &mut state.frontend,
            &state.match_state,
            &state.process_assets,
            crate::ui::lan_shell::LanChatLine {
                text: text.encode_utf16().collect(),
                color: u32::MAX,
            },
        );
    }
    true
}

/// Take a held result only while its ONE immutable MatchSession admission is
/// still current. Reducers may already have retired/replaced it while servicing
/// this boundary. The existing failure owner returns its lease exactly once.
fn take_after_network_service(
    slot: &mut Option<LoadingSession>,
    session: &crate::app::match_runtime::session::MatchSession,
) -> anyhow::Result<Option<LoadingSession>> {
    anyhow::ensure!(
        slot.as_ref().is_none_or(|loading| {
            match loading.stage.startup() {
                LoadingStartup::Network(admitted) => session.admits_network_start(admitted),
                _ => true,
            }
        }),
        "loading admission was retired before result handoff"
    );
    Ok(slot.take())
}

/// No ordinary reducer runs while synchronous phase/GPU/Rules borrows are held.
/// Once those borrows end, restore THIS session before normal HTTPS/UDP service
/// and before any consuming result handoff. Unrelated controls stayed in their
/// original queues, so a cancellation cannot be swallowed by a missing loader.
fn service_network_loading_boundary(
    state: &mut AppState,
    session: LoadingSession,
) -> Option<LoadingSession> {
    debug_assert!(state.frontend.loading_session.is_none());
    state.frontend.loading_session = Some(session);
    if state
        .match_state
        .session
        .network_return_context()
        .is_some_and(|context| {
            context.session_mode() == crate::app::match_runtime::session::SessionMode::Wol
        })
    {
        super::super::shell_wol::poll(state);
    }
    if network_loading_active(&state.frontend) {
        super::super::lan_service::poll(
            state,
            crate::app::match_runtime::frame_pacer::wall_clock_ms(),
        );
    }
    if !matches!(state.frontend.screen, GameScreen::Loading) {
        clear_loading_state(
            &mut state.renderer,
            &mut state.frontend,
            &mut state.process_assets,
        );
        return None;
    }
    if state.frontend.shell_exit.is_some() {
        return None; // The ordinary transition still owns this restored slot.
    }
    match take_after_network_service(
        &mut state.frontend.loading_session,
        &state.match_state.session,
    ) {
        Ok(session) => session,
        Err(error) => {
            fail_loading(state, LoadingFailurePolicy::RetryNetworkShell, error);
            None
        }
    }
}

fn prepare_session_for_loading(
    session: &mut crate::app::match_runtime::session::MatchSession,
    startup: &LoadingStartup,
) {
    match startup {
        LoadingStartup::Network(startup) => session.assert_network_start(startup),
        // The shell retains the recorded mode until its world is installed;
        // replay never admits a participant table or live connection.
        LoadingStartup::Replay {
            launch, game_mode, ..
        } => session.begin_replay(*game_mode, launch.session().local_player().name.clone()),
        LoadingStartup::AuthoredCampaignReplay { game_mode, .. } => {
            session.begin_replay(*game_mode, String::new())
        }
        startup => session.begin_offline(
            startup.session_mode(),
            startup
                .launch_session()
                .map(|launch| launch.local_player().name.clone()),
        ),
    }
}

/// VERA: a network match plays the switches its room agreed on
/// ([`MatchSession::network_personal_rules`]); the session holds this start
/// frozen. A replay applies the exact simulation switches recorded by its
/// blueprint; an offline game plays the player's own.
///
/// [`MatchSession::network_personal_rules`]: crate::app::match_runtime::session::MatchSession::network_personal_rules
fn play_startup_personal_rules(
    process_assets: &mut crate::app::process_assets::ProcessAssets,
    session: &crate::app::match_runtime::session::MatchSession,
    startup: &LoadingStartup,
) {
    let rules = match startup {
        LoadingStartup::Network(_) => session
            .network_personal_rules()
            .unwrap_or_else(|| Err("the start has no room setup".into()))
            .unwrap_or_else(|error| {
                // The room's agreement refused these before its start.
                log::error!("The room's rules do not read: {error}");
                Default::default()
            }),
        LoadingStartup::AuthoredCampaignReplay { .. } => Default::default(),
        LoadingStartup::Replay { launch, .. } => {
            launch.personal_rules().cloned().unwrap_or_default()
        }
        _ => return process_assets.use_own_personal_rules(),
    };
    process_assets.use_personal_rules(rules);
}

pub(crate) fn begin_loading(state: &mut AppState, request: LoadingRequest) {
    // Retiring the Radar FIFO can record an outgoing scenario service.
    crate::app::match_runtime::sim_tick::finish_threaded_frame(state);
    state
        .frontend
        .multiplayer_shell_runtime
        .begin_fresh_scenario_load();
    let wall_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
    crate::app::radar_movie::retire(
        &state.platform,
        &state.renderer,
        &state.diag,
        &state.frontend,
        &mut state.match_state,
        &state.process_assets,
        &mut state.audio,
        &state.persistence,
        wall_ms,
    );
    crate::app::chrono_screen::retire(
        &state.frontend.screen,
        &mut state.match_state,
        &mut state.audio,
        wall_ms,
    );
    // The shell writes GameMode before the loading screen (`0x0052E10F`
    // Skirmish, `0x0052E175` campaign); the session owner is live from here.
    prepare_session_for_loading(&mut state.match_state.session, request.startup());
    play_startup_personal_rules(
        &mut state.process_assets,
        &state.match_state.session,
        request.startup(),
    );
    let mut request = request.with_skirmish_colors(
        crate::app::state::active_rules(&state.frontend, &state.match_state)
            .map(crate::rules::ruleset::RuleSet::skirmish_colors),
    );
    if request.startup().session_mode() == crate::sim::scenario_session::ScenarioGameMode::Skirmish
        && request.startup().match_spec().rules().uses_room_options()
    {
        request
            .supplied_tiberium
            .get_or_insert(state.match_state.session.staged_tiberium());
    }
    request.the_team_enabled = state
        .frontend
        .main_menu_shell_state
        .typed_codes
        .the_team_enabled();
    let mut session = LoadingSession::from_request(request);
    session.job.ra2_dir = state
        .platform
        .game_config
        .as_ref()
        .map(|config| config.paths.ra2_dir.clone());
    // Resolve the backing fill from the live rules `[Colors]` schemes now that
    // `state.rules` is reachable (the native ctor only sees the launch session).
    if let (Some(native), Some(rules)) = (
        session.native.as_mut(),
        crate::app::state::active_rules(&state.frontend, &state.match_state),
    ) {
        // ReadScenario68479D..6847C9 projects raw HouseType+BC; ctor-only
        // countries retain -1. Only observer country selection maps to entry0.
        if let Some(launch) = session.stage.startup().launch_session() {
            let country = if launch.local_player().is_observer() {
                LaunchCountry::America
            } else {
                launch.local_slot().country
            };
            native.local_side_index = rules
                .country_name(country.index())
                .and_then(|id| rules.country_side_index(id))
                .map_or(-1, |side| i32::from(side.0));
        }
        native.resolve_player_colors(&rules.color_schemes, &rules.house_color_ramps);
    }
    let now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
    reset_loading_presentation(&mut state.renderer, &mut state.frontend);
    // A finished match keeps its presentation at the shell, and its entity
    // atlases are its largest GPU allocations (SHP pages up to the texture
    // limit). Nothing draws them once loading begins and the install replaces
    // them, so free them before this load builds its own.
    let outgoing = &mut state.match_state.match_presentation;
    outgoing.atlas_growth.cancel();
    outgoing.sprite_atlas = None;
    outgoing.zshape_skips.get_mut().forget_frames();
    outgoing.unit_atlas = None;
    replace_loading_attempt(
        &mut state.frontend.loading_session,
        &mut state.match_state.startup,
        &mut state.process_assets,
        &mut state.audio,
        session,
        now_ms,
    );
    crate::app::App::close_shell_menu_at_launch(state);
    state.frontend.screen = GameScreen::Loading;
}

/// Establish replacement admission before retiring the previous attempt's
/// resources. Successful resource retirement must leave this correlation live
/// for the installer's L0 acknowledgement.
fn replace_loading_attempt(
    slot: &mut Option<LoadingSession>,
    startup: &mut crate::app::match_runtime::startup::MatchStartup,
    assets: &mut crate::app::process_assets::ProcessAssets,
    audio: &mut crate::app::audio_runtime::AppAudioRuntime,
    next: LoadingSession,
    now_ms: u64,
) {
    let correlation = next.stage.startup().accepted().map(|s| s.correlation);
    startup.begin(correlation);
    retire_loading_attempt(slot, assets);
    *slot = Some(lease_loading_assets_and_play_loading_theme(
        assets, audio, next, now_ms,
    ));
    log::debug!(target: "vera20k::loading_attempt", "selected correlation={correlation:?}");
}

/// The shell -> scenario boundary of `begin_loading`: lease the process asset
/// manager into the loading job, then issue the LOADING theme through it.
///
/// Retail has one process-global MIX list and LoadFileFromMIX cache. Lease
/// that same manager through the loading job instead of reconstructing it
/// (F11 slot: Available -> Loading). `ScenarioClass__Start_Scenario @
/// 0x00683AB0` then plays `From_Name("LOADING")` (`0x00683D0F`) via
/// `Play_Song` (`0x00683D1A`) before `Read_Scenario` (`0x00684620`): a hard
/// replacement of the shell INTRO stream that loops (Repeat=yes) under the
/// loading screen until the post-read `Stop(1)` / `Queue_Song`. Native never
/// loses its MIX list across that boundary, so the request must resolve the
/// manager through the lease rather than the (now empty) resident slot.
fn lease_loading_assets_and_play_loading_theme(
    process_assets: &mut crate::app::process_assets::ProcessAssets,
    audio: &mut crate::app::audio_runtime::AppAudioRuntime,
    mut session: LoadingSession,
    now_ms: u64,
) -> LoadingSession {
    session.job.asset_manager = process_assets.lease_for_loading();
    // Ares683C70 needs the selected map INI. When the Rules are read the Ares
    // way its one playback occurs at the consuming Selected->Prepared
    // transition, before the Rules prefix reset.
    let ares = process_assets.reads_ares_way();
    if !ares && let Some(assets) = audio_service_asset_manager(process_assets, Some(&session)) {
        let _ = audio.play_theme("LOADING", assets, now_ms);
    }
    session
}

/// Installed Ares683C70/683D05 select from the already parsed map (campaign)
/// or the early process Rules/Country/Side authority (other sessions). Called
/// only by Selected->Prepared, before E_multi and later mode/map type writes.
fn play_ares_loading_theme(
    process_assets: &crate::app::process_assets::ProcessAssets,
    audio: &mut crate::app::audio_runtime::AppAudioRuntime,
    startup: &LoadingStartup,
    scenario: &crate::rules::ini_parser::IniFile,
    assets: &AssetManager,
    wall_ms: u64,
) {
    let names = if startup.campaign().is_some() {
        [
            scenario
                .section("Basic")
                .and_then(|section| section.read_name("LoadingTheme", 128))
                .map(str::to_owned),
            None,
        ]
    } else {
        startup
            .launch_session()
            .and_then(|launch| launch.humans.first())
            .and_then(|first| {
                process_assets
                    .native_rules()
                    .map(|owner| owner.loading_theme_names(first.slot.country.index()))
            })
            .unwrap_or([None, None])
    };
    audio.play_ares_loading_theme(names.iter().map(Option::as_deref), assets, wall_ms);
}

/// The asset manager the audio service polls Theme with: the resident
/// process manager, or the one leased to the loading job while the loading
/// screen owns it. `AudioSystem__Pump @ 0x00406F70` keeps reaching
/// `ThemeClass::AI` under the loading screen with the same process-global
/// MIX list, so the lease must not blind the poll (LOADING repeats there).
pub(crate) fn audio_service_asset_manager<'a>(
    process_assets: &'a crate::app::process_assets::ProcessAssets,
    loading_session: Option<&'a LoadingSession>,
) -> Option<&'a AssetManager> {
    process_assets
        .manager()
        .or_else(|| loading_session.and_then(loading_asset_manager))
}

/// Read-only projection of the loading presentation actually selected by this
/// owner. Diagnostic callers can compare observer art and source-node order
/// without reaching mutable launch/progress state or reconstructing its policy.
#[derive(Debug)]
pub(crate) struct NetworkLoadingPresentation<'a> {
    pub local_player_index: usize,
    pub art: Option<LoadingArtVariant>,
    pub participants: Vec<NetworkLoadingParticipantPresentation<'a>>,
    pub atlas_ready: bool,
}

#[derive(Debug)]
pub(crate) struct NetworkLoadingParticipantPresentation<'a> {
    pub label: &'a str,
    pub art: Option<LoadingArtVariant>,
}

pub(crate) fn network_loading_presentation(
    frontend: &crate::app::frontend::state::FrontendState,
) -> Option<NetworkLoadingPresentation<'_>> {
    let session = frontend.loading_session.as_ref()?;
    if !matches!(session.stage.startup(), LoadingStartup::Network(_)) {
        return None;
    }
    let native = session.native.as_ref()?;
    let NativeLoadingArt::StockOffline(art) = native.variant else {
        return None;
    };
    Some(NetworkLoadingPresentation {
        local_player_index: native.progress_row.local_player_index,
        art,
        participants: native
            .progress_row
            .participants
            .iter()
            .map(|row| NetworkLoadingParticipantPresentation {
                label: &row.label,
                art: row.variant,
            })
            .collect(),
        atlas_ready: native.atlas.is_some(),
    })
}

pub(crate) fn loading_map_name(
    frontend: &crate::app::frontend::state::FrontendState,
) -> Option<&str> {
    frontend
        .loading_session
        .as_ref()
        .map(|session| session.stage.startup().selected_map_file())
}

/// Immutable selected campaign for loading diagnostics; the stage retains
/// authority over its request through preparation and terminal disposition.
pub(crate) fn loading_campaign_startup(
    frontend: &crate::app::frontend::state::FrontendState,
) -> Option<&crate::app::match_bootstrap::PreparedCampaignStartup> {
    frontend
        .loading_session
        .as_ref()?
        .stage
        .startup()
        .campaign()
}

pub(crate) fn loading_map_source(
    frontend: &crate::app::frontend::state::FrontendState,
) -> Option<&crate::map::source::LoadedMapSource> {
    match &frontend.loading_session.as_ref()?.stage {
        LoadingStage::Prepared(prepared) => Some(prepared.initial.map_source()),
        LoadingStage::ConstructingScenario(constructing) => Some(constructing.load.map_source()),
        LoadingStage::Selected(_) => None,
        LoadingStage::PreparingPresentation { result }
        | LoadingStage::AwaitingPeers { result, .. }
        | LoadingStage::ReadyToInstall { result } => Some(&result.scenario.map_source),
    }
}

pub(crate) fn clear_loading_state(
    renderer: &mut crate::app::renderer_state::RendererState,
    frontend: &mut crate::app::frontend::state::FrontendState,
    process_assets: &mut crate::app::process_assets::ProcessAssets,
) {
    retire_loading_attempt(&mut frontend.loading_session, process_assets);
    reset_loading_presentation(renderer, frontend);
}

fn reset_loading_presentation(
    renderer: &mut crate::app::renderer_state::RendererState,
    frontend: &mut crate::app::frontend::state::FrontendState,
) {
    renderer.performance_overlay.clear_simulation_rate();
    frontend.loading_progress = LoadingProgressState::standard_skirmish();
}

fn retire_loading_attempt(
    slot: &mut Option<LoadingSession>,
    assets: &mut crate::app::process_assets::ProcessAssets,
) {
    if let Some(session) = slot.take() {
        session.retire(assets);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LoadingFailurePolicy {
    ReportNativeFailure,
    RetryCampaignShell,
    RetryNetworkShell,
    InstallGenericFallback,
}

fn retire_failed_loading_attempt(
    slot: &mut Option<LoadingSession>,
    startup: &mut crate::app::match_runtime::startup::MatchStartup,
    assets: &mut crate::app::process_assets::ProcessAssets,
    policy: LoadingFailurePolicy,
) -> LoadingFailurePolicy {
    retire_loading_attempt(slot, assets);
    if policy != LoadingFailurePolicy::InstallGenericFallback {
        startup.clear();
    }
    policy
}

fn is_native_loading_session(frontend: &crate::app::frontend::state::FrontendState) -> bool {
    frontend
        .loading_session
        .as_ref()
        .is_some_and(|session| session.native.is_some())
}

fn loading_failure_policy(
    frontend: &crate::app::frontend::state::FrontendState,
) -> LoadingFailurePolicy {
    frontend.loading_session.as_ref().map_or(
        LoadingFailurePolicy::InstallGenericFallback,
        LoadingSession::failure_policy,
    )
}

fn pump_loading(state: &mut AppState, show_progress: bool) -> anyhow::Result<LoadingPump> {
    let Some(session) = state.frontend.loading_session.take() else {
        return Ok(LoadingPump::Pending);
    };
    #[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
    let session = if matches!(session.stage.startup(), LoadingStartup::Network(_)) {
        let Some(mut session) = service_network_loading_boundary(state, session) else {
            return Ok(LoadingPump::Pending);
        };
        // Every online continuation, including atlas preparation and the final
        // handoff, resumes the original ACK drain before advancing another phase.
        let resumed = (|| -> anyhow::Result<bool> {
            if let Some(native) = session.native.as_mut()
                && let Some(mut network) = crate::app::lan_service::LoadingNetworkService::borrow(
                    &mut state.match_state.session,
                    state.match_state.lan_service.as_mut(),
                    state.platform.capture_client_size.is_some(),
                )?
            {
                return network.resume(&mut native.progress_row);
            }
            Ok(true)
        })();
        match resumed {
            Ok(true) => {}
            Ok(false) => {
                state.frontend.loading_session = Some(session);
                return Ok(LoadingPump::Pending);
            }
            Err(error) => {
                session.retire(&mut state.process_assets);
                return Ok(LoadingPump::Failed(error));
            }
        }
        session
    } else {
        session
    };
    if matches!(session.stage, LoadingStage::ConstructingScenario(_)) {
        return pump_map_construction(
            &state.platform,
            &state.renderer,
            &mut state.frontend,
            &mut state.match_state,
            &mut state.process_assets,
            &state.audio.theme,
            &state.persistence.options_profile,
            session,
            show_progress,
        );
    }
    if matches!(session.stage, LoadingStage::PreparingPresentation { .. }) {
        return pump_presentation(
            &state.platform,
            &state.renderer,
            &mut state.frontend,
            &mut state.match_state,
            &mut state.process_assets,
            session,
            show_progress,
        );
    }
    if matches!(session.stage, LoadingStage::ReadyToInstall { .. }) {
        return finish_network_load(state, session);
    }
    if matches!(session.stage, LoadingStage::AwaitingPeers { .. }) {
        return poll_network_load_wait(state, session);
    }
    let session = if matches!(session.stage.startup(), LoadingStartup::Network(_))
        && !cfg!(all(
            feature = "wasm-net",
            target_arch = "wasm32",
            target_os = "unknown"
        )) {
        let Some(session) = service_network_loading_boundary(state, session) else {
            return Ok(LoadingPump::Pending);
        };
        session
    } else {
        session
    };
    if session.native_pump_blocked() {
        session.retire(&mut state.process_assets);
        return Ok(LoadingPump::Failed(anyhow::anyhow!(
            "native loading renderer was not ready before the first loading pump"
        )));
    }

    if matches!(session.stage, LoadingStage::Selected(_)) {
        let network = match crate::app::lan_service::LoadingNetworkService::borrow(
            &mut state.match_state.session,
            state.match_state.lan_service.as_mut(),
            state.platform.capture_client_size.is_some(),
        ) {
            Ok(network) => network,
            Err(error) => {
                session.retire(&mut state.process_assets);
                return Ok(LoadingPump::Failed(error));
            }
        };
        return Ok(
            match prepare_loading_session(
                &mut state.process_assets,
                &mut state.audio,
                &mut state.frontend.random_map_retention,
                state
                    .match_state
                    .sim_runtime
                    .write_sim(SimWrite::Lifecycle)
                    .map(|runtime| &mut runtime.simulation),
                session,
                false,
                state
                    .platform
                    .game_config
                    .as_ref()
                    .map(|c| c.paths.ra2_dir.clone()),
                network,
            ) {
                Ok(session) => {
                    state.frontend.loading_session = Some(session);
                    LoadingPump::Pending
                }
                Err(err) => LoadingPump::Failed(err),
            },
        );
    }
    let LoadingSession {
        stage,
        mut native,
        mut job,
        ..
    } = session;
    let LoadingStage::Prepared(prepared) = stage else {
        unreachable!()
    };
    let PreparedScenarioLoad {
        request,
        initial,
        context: fresh_scenario_context,
        rules_prefix: mut prepared_rules_prefix,
    } = prepared;
    state
        .frontend
        .multiplayer_shell_runtime
        .observe_loaded_scenario_header(&initial.map_data().ini);
    log::debug!(target: "vera20k::loading_attempt", "remaining_load_begin");
    let mut continuing = None;
    let result = (|| -> anyhow::Result<Option<MapLoadResult>> {
        let native_theater_cache_mismatch = theater_cache_mismatch(
            state.match_state.loaded_map_source.is_some(),
            &state.match_state.match_presentation.theater_name,
            initial.theater_name(),
        );
        // All mid-load milestones (12..98) are emitted inside the loader; the
        // pump emits the terminal 100 once the result is ready. For the native
        // case we drive a RenderingProgressSink that synchronously repaints the
        // loading screen on each advancing milestone (gamemd's per-milestone
        // hidden-to-primary blit), so the bar visibly sweeps instead of
        // snapping once.
        //
        // Pre-copy the by-value pieces before borrowing so the disjoint
        // split-borrows (gpu/depth_view/batch shared,
        // native.progress &mut, native.atlas shared, request shared) all
        // hold simultaneously.
        let render_size = [
            state.renderer.gpu.config.width,
            state.renderer.gpu.config.height,
        ];
        let scale = loading_scale(&state.renderer.gpu);
        // The pre-parse swallowed the loader's raw 8 so it could not present
        // before the first frame; hand it over now for either native cadence.
        if let Some(native) = native.as_mut()
            && native
                .progress_cadence
                .prepares_scenario_before_first_frame()
        {
            advance_and_present_native_progress(
                show_progress,
                &state.renderer.gpu,
                &state.renderer.shell_surface_presenter,
                &state.renderer.depth_view,
                &state.renderer.batch_renderer,
                &state.renderer.bit_font,
                &state.renderer.performance_overlay,
                native,
                8,
                render_size,
                scale,
                crate::app::lan_service::LoadingNetworkService::borrow(
                    &mut state.match_state.session,
                    state.match_state.lan_service.as_mut(),
                    state.platform.capture_client_size.is_some(),
                )?,
            )?;
        }
        let startup = request.startup;
        if !state.process_assets.has_native_rules() {
            anyhow::bail!("loading requires the process-resident native Rules owner");
        }
        let asset_manager = job
            .asset_manager
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("loading job lost its process asset manager"))?;
        let shared_cell_dummy = state.process_assets.shared_cell_dummy.clone();
        let (native_rules_owner, tile_variant_selector_cache, audio) =
            state.process_assets.native_rules_mut_with_tile_cache();
        let native_rules_owner =
            native_rules_owner.expect("native Rules availability checked before split borrow");
        // The outgoing scene no longer advances. Offline loads return its
        // live Type and ambient Rules writes before Full_Init's prefix Process.
        // Network loads already installed a cold registry for the new match.
        // Type reset deletes Types but retains the ambient scalar defaults.
        // Only Rules read the Ares way bind a prefix, which already returned them.
        let should_retire_outgoing = prepared_rules_prefix.is_none();
        if !initial.has_generated_rules()
            && !startup.session_mode().is_network()
            && should_retire_outgoing
            && let Some(runtime) = state.match_state.sim_runtime.write_sim(SimWrite::Lifecycle)
        {
            crate::app::process_assets::transfer_outgoing_simulation_rules(
                native_rules_owner,
                &mut runtime.simulation,
            )?;
        }
        let the_team_enabled = state
            .frontend
            .main_menu_shell_state
            .typed_codes
            .the_team_enabled();
        let load_result = {
            let network = crate::app::lan_service::LoadingNetworkService::borrow(
                &mut state.match_state.session,
                state.match_state.lan_service.as_mut(),
                state.platform.capture_client_size.is_some(),
            )?;
            let mut progress = LoadingPhaseProgress::select(
                native.as_mut(),
                native_theater_cache_mismatch,
                show_progress,
                network,
                |native, network| {
                    RenderingProgressSink::new(
                        native,
                        network,
                        &state.renderer.gpu,
                        &state.renderer.shell_surface_presenter,
                        &state.renderer.depth_view,
                        &state.renderer.batch_renderer,
                        &state.renderer.bit_font,
                        &state.renderer.performance_overlay,
                        render_size,
                        scale,
                    )
                },
            );
            if matches!(startup, LoadingStartup::Network(_))
                && !cfg!(all(
                    feature = "wasm-net",
                    target_arch = "wasm32",
                    target_os = "unknown"
                ))
            {
                init::load_map_from_initial(
                    &state.renderer.gpu,
                    &state.renderer.batch_renderer,
                    asset_manager,
                    initial,
                    startup,
                    fresh_scenario_context,
                    progress.native_theater_cache_mismatch,
                    progress.runtime_color_scheme_count,
                    the_team_enabled,
                    native_rules_owner,
                    &mut prepared_rules_prefix,
                    audio,
                    state.audio.theme.catalog(),
                    shared_cell_dummy,
                    tile_variant_selector_cache,
                    &mut progress.sink,
                    state.persistence.options_profile.house_color_strength,
                )
                .map(Some)
            } else {
                let mut load = init::FreshMapLoadJob::new(
                    initial,
                    startup,
                    fresh_scenario_context,
                    progress.native_theater_cache_mismatch,
                    progress.runtime_color_scheme_count,
                );
                let result = load.step(init::FreshMapLoadServices {
                    gpu: &state.renderer.gpu,
                    batch: &state.renderer.batch_renderer,
                    asset_manager,
                    native_rules_owner,
                    the_team_enabled,
                    prepared_noncampaign_prefix: &mut prepared_rules_prefix,
                    audio,
                    themes: state.audio.theme.catalog(),
                    shared_cell_dummy,
                    tile_variant_selector_cache,
                    progress: &mut progress.sink,
                    house_color_strength: state.persistence.options_profile.house_color_strength,
                })?;
                if result.is_none() {
                    continuing = Some(load);
                }
                Ok(result)
            }
        };

        match load_result {
            Ok(Some(mut result)) => {
                if result.pending_atlases.is_none() {
                    publish_load_complete(
                        &state.platform,
                        &state.renderer,
                        &mut state.match_state,
                        show_progress,
                        &mut native,
                        &mut result,
                    )?;
                }
                result.asset_manager = job.asset_manager.take();
                Ok(Some(result))
            }
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    })();
    // init borrows the ticket until its actual Rules reset boundary. Any
    // earlier asset/map validation failure leaves it here for explicit retire;
    // a consumed ticket's load_scenario already restored its partial Live state.
    if continuing.is_none() {
        retire_rules_prefix(prepared_rules_prefix.take(), &mut state.process_assets);
    }
    Ok(match result {
        Ok(None) => {
            state.frontend.loading_session = Some(LoadingSession {
                stage: LoadingStage::ConstructingScenario(ConstructingScenarioLoad {
                    load: continuing.expect("pending native construction"),
                    rules_prefix: prepared_rules_prefix,
                }),
                native,
                job,
                next_frame: NextLoadingFrame::Later,
                install_after_ms: None,
            });
            LoadingPump::Pending
        }
        Ok(Some(result)) => {
            if result.pending_atlases.is_some() {
                state.frontend.loading_session = Some(LoadingSession {
                    stage: LoadingStage::PreparingPresentation {
                        result: Box::new(result),
                    },
                    native,
                    job,
                    next_frame: NextLoadingFrame::Later,
                    install_after_ms: None,
                });
                return Ok(LoadingPump::Pending);
            }
            // VERA adaptation: a returning process has nobody to wait for, the
            // others left their own loading long ago and report nothing more.
            if matches!(result.scenario.startup, LoadingStartup::Network(_))
                && !crate::app::lan_service::online_restore(&state.match_state)
            {
                let native = native.expect("network load retains its source progress rows");
                let now = (crate::app::match_runtime::frame_pacer::wall_clock_ms() >> 4) as i32;
                let wait =
                    NetworkLoadWait::new(now, native.progress_row.total_fraction(&native.progress));
                //The terminal callback above already published the effective
                //percentage. Wait684370 owns subsequent100/200 resends.
                return poll_network_load_wait(
                    state,
                    LoadingSession {
                        stage: LoadingStage::AwaitingPeers {
                            result: Box::new(result),
                            wait,
                        },
                        native: Some(native),
                        job,
                        next_frame: NextLoadingFrame::Later,
                        install_after_ms: None,
                    },
                );
            } else {
                LoadingPump::Finished {
                    result: Box::new(result),
                    timed_out: Vec::new(),
                }
            }
        }
        Err(err) => {
            job.retire(&mut state.process_assets);
            LoadingPump::Failed(err)
        }
    })
}

fn pump_map_construction(
    platform: &crate::app::PlatformState,
    renderer: &crate::app::renderer_state::RendererState,
    frontend: &mut crate::app::frontend::state::FrontendState,
    match_state: &mut crate::app::match_runtime::state::MatchState,
    process_assets: &mut crate::app::process_assets::ProcessAssets,
    theme: &crate::audio::theme::ThemeRuntime,
    options_profile: &crate::app::persistence::options_profile::RetailOptionsProfile,
    mut session: LoadingSession,
    show_progress: bool,
) -> anyhow::Result<LoadingPump> {
    let render_size = [renderer.gpu.config.width, renderer.gpu.config.height];
    let scale = loading_scale(&renderer.gpu);
    let step = (|| {
        let LoadingStage::ConstructingScenario(constructing) = &mut session.stage else {
            unreachable!()
        };
        let asset_manager = session
            .job
            .asset_manager
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("native construction lost its asset lease"))?;
        let shared_cell_dummy = process_assets.shared_cell_dummy.clone();
        let (rules, tile_variant_selector_cache, audio) =
            process_assets.native_rules_mut_with_tile_cache();
        let native_rules_owner =
            rules.ok_or_else(|| anyhow::anyhow!("native construction lost its Rules owner"))?;
        let the_team_enabled = frontend
            .main_menu_shell_state
            .typed_codes
            .the_team_enabled();
        #[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
        let network = crate::app::lan_service::LoadingNetworkService::borrow(
            &mut match_state.session,
            match_state.lan_service.as_mut(),
            platform.capture_client_size.is_some(),
        )?;
        #[cfg(not(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown")))]
        let network = None;
        let mut progress = LoadingPhaseProgress::select(
            session.native.as_mut(),
            constructing.load.theater_cache_mismatch(),
            show_progress,
            network,
            |native, network| {
                RenderingProgressSink::new(
                    native,
                    network,
                    &renderer.gpu,
                    &renderer.shell_surface_presenter,
                    &renderer.depth_view,
                    &renderer.batch_renderer,
                    &renderer.bit_font,
                    &renderer.performance_overlay,
                    render_size,
                    scale,
                )
            },
        );
        constructing.load.step(init::FreshMapLoadServices {
            gpu: &renderer.gpu,
            batch: &renderer.batch_renderer,
            asset_manager,
            native_rules_owner,
            the_team_enabled,
            prepared_noncampaign_prefix: &mut constructing.rules_prefix,
            audio,
            themes: theme.catalog(),
            shared_cell_dummy,
            tile_variant_selector_cache,
            progress: &mut progress.sink,
            house_color_strength: options_profile.house_color_strength,
        })
    })();
    match step {
        Err(error) => {
            session.retire(process_assets);
            Ok(LoadingPump::Failed(error))
        }
        Ok(None) => {
            frontend.loading_session = Some(session);
            Ok(LoadingPump::Pending)
        }
        Ok(Some(mut result)) => {
            if let LoadingStage::ConstructingScenario(constructing) = &mut session.stage {
                retire_rules_prefix(constructing.rules_prefix.take(), process_assets);
            }
            if result.pending_atlases.is_none() {
                if let Err(error) = publish_load_complete(
                    platform,
                    renderer,
                    match_state,
                    show_progress,
                    &mut session.native,
                    &mut result,
                ) {
                    session.retire(process_assets);
                    return Ok(LoadingPump::Failed(error));
                }
            }
            result.asset_manager = session.job.asset_manager.take();
            if result.pending_atlases.is_some() {
                session.stage = LoadingStage::PreparingPresentation {
                    result: Box::new(result),
                };
                frontend.loading_session = Some(session);
                Ok(LoadingPump::Pending)
            } else {
                #[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
                if matches!(result.scenario.startup, LoadingStartup::Network(_)) {
                    frontend.loading_session = Some(LoadingSession::await_browser_network_peers(
                        Box::new(result),
                        session.native,
                        session.job,
                    ));
                    return Ok(LoadingPump::Pending);
                }
                Ok(LoadingPump::Finished {
                    result: Box::new(result),
                    timed_out: Vec::new(),
                })
            }
        }
    }
}

fn publish_load_complete(
    platform: &crate::app::PlatformState,
    renderer: &crate::app::renderer_state::RendererState,
    match_state: &mut crate::app::match_runtime::state::MatchState,
    show_progress: bool,
    native: &mut Option<NativeLoadingScreenState>,
    result: &mut MapLoadResult,
) -> anyhow::Result<()> {
    let render_size = [renderer.gpu.config.width, renderer.gpu.config.height];
    let scale = loading_scale(&renderer.gpu);
    if let Some(native) = native.as_mut() {
        result.scenario.campaign_names = native.campaign_names.take();
        let terminal_raw_percent = native.progress_cadence.terminal_raw_percent();
        advance_and_present_native_progress(
            show_progress,
            &renderer.gpu,
            &renderer.shell_surface_presenter,
            &renderer.depth_view,
            &renderer.batch_renderer,
            &renderer.bit_font,
            &renderer.performance_overlay,
            native,
            terminal_raw_percent,
            render_size,
            scale,
            crate::app::lan_service::LoadingNetworkService::borrow(
                &mut match_state.session,
                match_state.lan_service.as_mut(),
                platform.capture_client_size.is_some(),
            )?,
        )?;
    }
    Ok(())
}

fn pump_presentation(
    platform: &crate::app::PlatformState,
    renderer: &crate::app::renderer_state::RendererState,
    frontend: &mut crate::app::frontend::state::FrontendState,
    match_state: &mut crate::app::match_runtime::state::MatchState,
    process_assets: &mut crate::app::process_assets::ProcessAssets,
    mut session: LoadingSession,
    show_progress: bool,
) -> anyhow::Result<LoadingPump> {
    let LoadingStage::PreparingPresentation { result } = &mut session.stage else {
        unreachable!()
    };
    let polled = result
        .pending_atlases
        .as_mut()
        .expect("pending presentation owner")
        .poll(&renderer.gpu, &mut result.presentation);
    match polled {
        Err(error) => {
            session.retire(process_assets);
            return Ok(LoadingPump::Failed(error));
        }
        Ok(false) => {
            frontend.loading_session = Some(session);
            return Ok(LoadingPump::Pending);
        }
        Ok(true) => {
            result.pending_atlases = None;
        }
    }
    if let Err(error) = publish_load_complete(
        platform,
        renderer,
        match_state,
        show_progress,
        &mut session.native,
        result,
    ) {
        session.retire(process_assets);
        return Ok(LoadingPump::Failed(error));
    }
    let LoadingStage::PreparingPresentation { result } = session.stage else {
        unreachable!()
    };
    #[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
    if matches!(result.scenario.startup, LoadingStartup::Network(_)) {
        frontend.loading_session = Some(LoadingSession::await_browser_network_peers(
            result,
            session.native,
            session.job,
        ));
        return Ok(LoadingPump::Pending);
    }
    Ok(LoadingPump::Finished {
        result,
        timed_out: Vec::new(),
    })
}

fn ensure_job_asset_manager(
    platform: &crate::app::PlatformState,
    frontend: &mut crate::app::frontend::state::FrontendState,
    process_assets: &mut crate::app::process_assets::ProcessAssets,
) -> anyhow::Result<()> {
    let Some(mut session) = frontend.loading_session.take() else {
        return Ok(());
    };
    let result = ensure_session_job_asset_manager(
        process_assets,
        &mut session,
        platform
            .game_config
            .as_ref()
            .map(|c| c.paths.ra2_dir.clone()),
    );
    frontend.loading_session = Some(session);
    result
}

fn loading_asset_manager(session: &LoadingSession) -> Option<&AssetManager> {
    match &session.stage {
        LoadingStage::PreparingPresentation { result }
        | LoadingStage::AwaitingPeers { result, .. }
        | LoadingStage::ReadyToInstall { result } => result.asset_manager.as_ref(),
        _ => session.job.asset_manager.as_ref(),
    }
}

fn ensure_session_job_asset_manager(
    process_assets: &mut crate::app::process_assets::ProcessAssets,
    session: &mut LoadingSession,
    configured_ra2_dir: Option<PathBuf>,
) -> anyhow::Result<()> {
    if session.job.ra2_dir.is_none() {
        session.job.ra2_dir = Some(
            configured_ra2_dir
                .ok_or_else(|| anyhow::anyhow!("missing game config for loading job assets"))?,
        );
    }
    if session.job.asset_manager.is_none() {
        let asset_manager = if let Some(asset_manager) = process_assets.lease_for_loading() {
            asset_manager
        } else {
            // Warn only when a manager actually existed and its lease was
            // lost — reconstructing then loses the sticky CRC cache and
            // theater identity. An asset-less startup (no retail archives)
            // has nothing to lose and stays quiet.
            if process_assets.is_leased() {
                log::warn!(
                    "loading job reconstructs an AssetManager; process-sticky \
                     MIX cache and theater identity restart"
                );
            }
            process_assets.note_lease_ended_without_return();
            let rebuilt = process_assets.reconstruct_manager(
                session
                    .job
                    .ra2_dir
                    .as_deref()
                    .expect("RA2 directory was initialized above"),
            )?;
            // The rebuilt manager is the game's from here on, so its answer
            // is the launch's again, whatever another command published since.
            rebuilt.open_for_play();
            rebuilt
        };
        session.job.asset_manager = Some(asset_manager);
    }
    // The job owns the manager before this fallible step so retirement returns
    // it on failure. A recovered process must have Rules before request.prepare.
    process_assets
        .initialize_sources_if_needed(
            session
                .job
                .asset_manager
                .as_ref()
                .expect("asset setup stores manager"),
        )
        .map_err(anyhow::Error::msg)?;
    Ok(())
}

fn poll_network_load_wait(
    state: &mut AppState,
    session: LoadingSession,
) -> anyhow::Result<LoadingPump> {
    let Some(mut session) = service_network_loading_boundary(state, session) else {
        return Ok(LoadingPump::Pending);
    };
    let LoadingStage::AwaitingPeers { wait, .. } = &mut session.stage else {
        unreachable!()
    };
    let native = session.native.as_ref().expect("network source progress");
    let now = (crate::app::match_runtime::frame_pacer::wall_clock_ms() >> 4) as i32;
    let poll = wait.poll(now, native.progress_row.total_fraction(&native.progress));
    if matches!(
        poll,
        NetworkLoadPoll::ReportProgress | NetworkLoadPoll::Complete
    ) {
        state.match_state.session.report_network_loading_progress(
            native
                .progress_cadence
                .effective_percent(native.progress_cadence.terminal_raw_percent())
                as i32,
        );
    }
    if poll == NetworkLoadPoll::Complete {
        let LoadingStage::AwaitingPeers { result, .. } = session.stage else {
            unreachable!()
        };
        session.stage = LoadingStage::ReadyToInstall { result };
        // Captures keep their timing; players see the full bar a moment.
        if state.platform.capture_client_size.is_none() {
            session.install_after_ms = Some(
                crate::app::match_runtime::frame_pacer::wall_clock_ms() + NETWORK_LOADED_HOLD_MS,
            );
        }
        return finish_network_load(state, session);
    }
    if poll == NetworkLoadPoll::TimedOut {
        let timed_out = native.progress_row.incomplete_remote_indices();
        if state
            .match_state
            .session
            .retire_cooperative_start_on_timeout(&timed_out)
        {
            // Retire the held result and its asset lease through the normal
            // failure path before the world can be installed.
            state.frontend.loading_session = Some(session);
            let text = state.process_assets.csf_text("TXT_SYSTEM_NOT_RESPONDING");
            return Ok(LoadingPump::Failed(anyhow::anyhow!(text)));
        }
        let LoadingStage::AwaitingPeers { result, .. } = session.stage else {
            unreachable!()
        };
        Ok(LoadingPump::Finished { result, timed_out })
    } else {
        state.frontend.loading_session = Some(session);
        Ok(LoadingPump::Pending)
    }
}

/// Consuming handoff after the native wait has completed exactly once. The
/// loaded-screen hold uses this existing stage on later frames without
/// re-polling timers, regenerating the scenario or publishing progress again.
fn finish_network_load(
    state: &mut AppState,
    session: LoadingSession,
) -> anyhow::Result<LoadingPump> {
    let Some(session) = service_network_loading_boundary(state, session) else {
        return Ok(LoadingPump::Pending);
    };
    if session.holds_loaded_screen(crate::app::match_runtime::frame_pacer::wall_clock_ms()) {
        state.frontend.loading_session = Some(session);
        return Ok(LoadingPump::Pending);
    }
    let LoadingStage::ReadyToInstall { result } = session.stage else {
        unreachable!()
    };
    Ok(LoadingPump::Finished {
        result,
        timed_out: Vec::new(),
    })
}

impl LoadingJob {
    fn retire(self, process_assets: &mut crate::app::process_assets::ProcessAssets) {
        if let Some(manager) = self.asset_manager {
            process_assets.return_from_loading(manager);
        }
    }
}

/// Both entry timings use the same consuming preparation. Only the progress
/// sink differs: prepaint must swallow raw8 until the first frame is presented.
fn prepare_loading_session(
    process_assets: &mut crate::app::process_assets::ProcessAssets,
    audio: &mut crate::app::audio_runtime::AppAudioRuntime,
    mapgen_retention: &mut crate::app::shell_random_map::RandomMapGenerationRetention,
    mut resident: Option<&mut crate::sim::world::Simulation>,
    mut session: LoadingSession,
    before_first_frame: bool,
    configured_ra2_dir: Option<PathBuf>,
    network: Option<crate::app::lan_service::LoadingNetworkService<'_>>,
) -> anyhow::Result<LoadingSession> {
    if let Err(err) =
        ensure_session_job_asset_manager(process_assets, &mut session, configured_ra2_dir)
    {
        session.retire(process_assets);
        return Err(err);
    }
    // A recovered assetless launch selects Theme before request preparation
    // and scenario/theater archive changes. The process holder keeps an
    // already selected catalog, including an empty one, across every attempt.
    if let Some(assets) = loading_asset_manager(&session) {
        audio.initialize_theme(assets);
    }
    let LoadingSession {
        stage,
        mut native,
        mut job,
        next_frame,
        install_after_ms,
    } = session;
    let stage = match stage {
        stage @ (LoadingStage::ConstructingScenario(_)
        | LoadingStage::PreparingPresentation { .. }
        | LoadingStage::AwaitingPeers { .. }
        | LoadingStage::ReadyToInstall { .. }) => stage,
        LoadingStage::Prepared(prepared) => LoadingStage::Prepared(prepared),
        LoadingStage::Selected(request) => {
            let ra2_dir = job
                .ra2_dir
                .clone()
                .expect("asset setup stores RA2 directory");
            let assets = job
                .asset_manager
                .as_mut()
                .expect("asset setup stores manager");
            if request.startup().campaign().is_some() {
                // ClearScene685609 rereads campaign sources before the second
                // Full_Init prefix; the selected StartScenario map is already
                // an immutable request, while retained catalog fields update.
                process_assets.reload_campaigns(assets);
            }
            let ares = process_assets.reads_ares_way();
            if ares && let Some(native) = native.as_mut() {
                let early = process_assets
                    .native_rules()
                    .ok_or_else(|| {
                        anyhow::anyhow!("loading requires the process-resident Rules owner")
                    })
                    .and_then(|owner| {
                        bind_early_country_loading(native, request.startup(), owner, assets)
                    });
                if let Err(err) = early {
                    job.retire(process_assets);
                    return Err(err);
                }
            }
            // VERA: the mod's map additions, read once a start, for the
            // matches they reach.
            let additions = if crate::app::mod_additions::reach(request.startup()) {
                process_assets.map_additions(assets)
            } else {
                Ok(None)
            };
            let prepared = match additions {
                Err(err) => Err(anyhow::anyhow!(err)),
                Ok(additions) => {
                    let additions = additions.as_deref();
                    if !before_first_frame && let Some(native) = native.as_mut() {
                        let mut sink = GatedProgressSink {
                            progress: &mut native.progress,
                            cadence: native.progress_cadence,
                            progress_row: &mut native.progress_row,
                            network,
                        };
                        request.prepare(
                            ra2_dir,
                            assets,
                            process_assets.native_rules_mut_with_tile_cache().0,
                            additions,
                            mapgen_retention,
                            resident.as_deref_mut(),
                            &mut sink,
                        )
                    } else {
                        request.prepare(
                            ra2_dir,
                            assets,
                            process_assets.native_rules_mut_with_tile_cache().0,
                            additions,
                            mapgen_retention,
                            resident.as_deref_mut(),
                            &mut NoopProgressSink,
                        )
                    }
                }
            };
            match prepared {
                Ok(mut prepared) => {
                    if ares {
                        play_ares_loading_theme(
                            process_assets,
                            audio,
                            prepared.request.startup(),
                            &prepared.initial.map_data().ini,
                            assets,
                            crate::app::match_runtime::frame_pacer::wall_clock_ms(),
                        );
                    }
                    if ares
                        && let Some(native) = native.as_mut()
                        && let Err(err) = prepare_country_loading_prefix(
                            process_assets,
                            resident,
                            &mut prepared,
                            native,
                        )
                    {
                        prepared.cancel_rules_prefix(process_assets);
                        job.retire(process_assets);
                        return Err(err);
                    }
                    LoadingStage::Prepared(prepared)
                }
                Err(err) => {
                    job.retire(process_assets);
                    return Err(err);
                }
            }
        }
    };
    log::debug!(target: "vera20k::loading_attempt", "prepared before_first_frame={before_first_frame}");
    Ok(LoadingSession {
        stage,
        native,
        job,
        next_frame,
        install_after_ms,
    })
}

/// Resolve native Scenario inputs before constructing the first loading frame.
///
/// Full Init runs both selected-mode callbacks before DrawLoadingScreen. Fixed
/// maps additionally derive their preview from the parsed scenario; random maps
/// use `RandMap.img` pixels but still require regenerated header data and the
/// accepted staged starts. The loader's raw 8 is intentionally swallowed here
/// and visibly handed off only after the first frame has been presented.
fn prepare_scenario_initial_before_first_frame(
    platform: &crate::app::PlatformState,
    frontend: &mut crate::app::frontend::state::FrontendState,
    sim_runtime: &mut crate::app::match_runtime::sim_host::SimHost,
    process_assets: &mut crate::app::process_assets::ProcessAssets,
    audio: &mut crate::app::audio_runtime::AppAudioRuntime,
) -> anyhow::Result<()> {
    let should_prepare = frontend
        .loading_session
        .as_ref()
        .and_then(|session| session.native.as_ref())
        .is_some_and(|native| {
            native
                .progress_cadence
                .prepares_scenario_before_first_frame()
        })
        && frontend
            .loading_session
            .as_ref()
            .is_some_and(|session| matches!(session.stage, LoadingStage::Selected(_)));
    if !should_prepare {
        return Ok(());
    }

    let Some(session) = frontend.loading_session.take() else {
        return Ok(());
    };
    frontend.loading_session = Some(prepare_loading_session(
        process_assets,
        audio,
        &mut frontend.random_map_retention,
        sim_runtime
            .write_sim(SimWrite::Lifecycle)
            .map(|runtime| &mut runtime.simulation),
        session,
        true,
        platform
            .game_config
            .as_ref()
            .map(|c| c.paths.ra2_dir.clone()),
        None,
    )?);
    Ok(())
}

/// Decode the random-map preview bitmap written by the random-map setup dialog.
///
/// This is the whole preview source for a random-map load: gamemd reads the same
/// file rather than deriving a preview from the scenario. A missing or unreadable
/// file omits the preview layer; every other layer still draws.
fn decode_random_map_loading_preview(
    ra2_dir: &Path,
    source: Option<&crate::map::source::LoadedMapSource>,
) -> Option<DecodedPreview> {
    let path = match source {
        Some(crate::map::source::LoadedMapSource::Loose { path, .. }) => path.clone(),
        _ => crate::map::source::random_map_dir(ra2_dir)
            .join(crate::map::skirmish_scenarios::RANDMAP_PREVIEW),
    };
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) => {
            log::warn!(
                "Loading screen: no random-map preview at {} ({err})",
                path.display()
            );
            return None;
        }
    };
    if let Some(expected) = source.and_then(crate::map::source::LoadedMapSource::physical_identity)
        && expected
            != (
                bytes.len(),
                crate::util::sha256::sha256_hex(&bytes).as_str(),
            )
    {
        return None;
    }
    let pcx = match PcxFile::from_bytes(&bytes) {
        Ok(pcx) => pcx,
        Err(err) => {
            log::warn!(
                "Loading screen: cannot decode random-map preview {} ({err})",
                path.display()
            );
            return None;
        }
    };
    Some(DecodedPreview {
        width: u32::from(pcx.width),
        height: u32::from(pcx.height),
        rgba: pcx.to_rgba(None),
    })
}

/// Resolve the assigned start waypoints the marker layer draws for a selected map.
pub(crate) fn selected_map_start_assignments(
    launch_session: &SkirmishLaunchSession,
    projection: Option<&StockOfflinePrefixProjection>,
) -> Vec<LoadingStartAssignment> {
    let Some(projection) = projection else {
        return Vec::new();
    };
    projection
        .start_table()
        .iter()
        .enumerate()
        .filter_map(|(start_index, participant_index)| {
            let participant_index = (*participant_index)?;
            projection.final_gathered_starts().get(start_index)?;
            let slots = crate::sim::scenario_bootstrap::normalized_launch_slots(launch_session);
            let slot = slots.get(participant_index)?;
            // A shared House is one slot; its marker is Local for each of
            // its players, and AI slots follow the human Houses.
            let participant = match slot.source_human_index() {
                Some(_) if slot.is_controlled_by(launch_session.local_player_index) => {
                    LoadingParticipantId::Local
                }
                Some(index) => LoadingParticipantId::Human(index),
                None => LoadingParticipantId::Opponent(
                    participant_index - slots.iter().filter(|slot| slot.is_human).count(),
                ),
            };
            let color_priority = slot.color_index;
            Some(LoadingStartAssignment {
                start_index: u32::try_from(start_index).ok()?,
                participant,
                color_priority,
            })
        })
        .collect()
}

/// Build the loading composition for either map kind.
///
/// gamemd branches only the preview holder on the random-map flag; the four text
/// layers sit after that branch and are drawn for both kinds. Splitting the
/// snapshot on the branch (as this used to) dropped the country name, the
/// special-unit line, the briefing and "Loading..." from every random-map load.
fn ensure_loading_composition_snapshot(
    gpu: &GpuContext,
    frontend: &mut crate::app::frontend::state::FrontendState,
    process_assets: &crate::app::process_assets::ProcessAssets,
) {
    let snapshot = {
        let Some(session) = frontend.loading_session.as_ref() else {
            return;
        };
        let Some(native) = session.native.as_ref() else {
            return;
        };
        if native.variant == NativeLoadingArt::Campaign {
            return;
        }
        if native.composition.is_some() {
            return;
        }
        let LoadingStage::Prepared(prepared) = &session.stage else {
            return;
        };
        if prepared.request.startup().authored_campaign().is_some() {
            return;
        }
        let context = &prepared.context;
        let initial = &prepared.initial;
        let launch_session = context.stock_offline_launch().session();
        let projection = context.stock_offline_projection();
        let render_size = [gpu.config.width, gpu.config.height];
        let scale = loading_scale(gpu);
        match native.progress_cadence {
            NativeLoadingProgressCadence::SelectedMap => {
                let assignments = selected_map_start_assignments(launch_session, Some(projection));
                build_loading_composition(
                    initial.map_data(),
                    launch_session,
                    process_assets.csf.as_ref(),
                    render_size,
                    scale,
                    &assignments,
                )
            }
            NativeLoadingProgressCadence::RandomMapHalved => {
                let preview = session.job.ra2_dir.as_deref().and_then(|root| {
                    let preview = match session.stage.startup() {
                        LoadingStartup::Network(startup) => startup.random_map_preview(),
                        _ => None,
                    };
                    decode_random_map_loading_preview(root, preview)
                });
                let assignments = selected_map_start_assignments(launch_session, Some(projection));
                build_random_map_loading_composition(
                    launch_session,
                    process_assets.csf.as_ref(),
                    render_size,
                    scale,
                    preview,
                    initial.map_data(),
                    projection.active_scenario_waypoints(),
                    &assignments,
                )
            }
        }
    };

    if let Some(native) = frontend
        .loading_session
        .as_mut()
        .and_then(|session| session.native.as_mut())
    {
        let snapshot = {
            let mut snapshot = snapshot;
            if let Some(country) = native.late_country_loading.as_ref() {
                snapshot.bind_country_loading(country, process_assets.csf.as_ref());
            }
            snapshot
        };
        native.composition = Some(snapshot);
    }
}

fn theater_cache_mismatch(
    has_successfully_loaded_map: bool,
    cached_theater: &str,
    requested_theater: &str,
) -> bool {
    !has_successfully_loaded_map || !cached_theater.eq_ignore_ascii_case(requested_theater)
}

/// Derive the verified dynamic theater values from the native runtime scheme count.
///
/// Native relies on `N == 0 || N >= 13`; counts 1..12 would divide by zero.
/// Rust treats those malformed, non-stock counts like an empty dynamic loop.
pub(crate) fn theater_ramp_changed_values(runtime_color_scheme_count: usize) -> Vec<u32> {
    if runtime_color_scheme_count < 13 {
        return Vec::new();
    }

    let quotient = runtime_color_scheme_count / 13;
    let mut previous = 12;
    let mut emitted = Vec::new();
    for scheme_index in 0..runtime_color_scheme_count {
        let candidate = ((scheme_index / quotient).min(13) + 12) as u32;
        if candidate != previous {
            emitted.push(candidate);
            previous = candidate;
        }
    }
    emitted
}

pub(crate) fn ensure_native_loading_atlas(
    platform: &crate::app::PlatformState,
    renderer: &crate::app::renderer_state::RendererState,
    frontend: &mut crate::app::frontend::state::FrontendState,
    match_state: &mut crate::app::match_runtime::state::MatchState,
    process_assets: &mut crate::app::process_assets::ProcessAssets,
    audio: &mut crate::app::audio_runtime::AppAudioRuntime,
) -> anyhow::Result<()> {
    if !is_native_loading_session(frontend) {
        return Ok(());
    }
    if frontend
        .loading_session
        .as_ref()
        .and_then(|session| session.native.as_ref())
        .is_some_and(|native| native.atlas.is_some() || native.suppresses_chrome())
    {
        return Ok(());
    }
    if frontend
        .loading_session
        .as_ref()
        .and_then(loading_asset_manager)
        .is_none()
    {
        ensure_job_asset_manager(platform, frontend, process_assets)?;
    }
    let loading_archives_ready = frontend
        .loading_session
        .as_mut()
        .and_then(|session| session.job.asset_manager.as_mut())
        .ok_or_else(|| {
            anyhow::anyhow!("native loading job has no asset manager after initialization")
        })?
        .register_loading_archives()?;
    if !loading_archives_ready {
        return Err(anyhow::anyhow!(
            "native loading archives LOADMD.MIX and LOAD.MIX are unavailable"
        ));
    }
    prepare_scenario_initial_before_first_frame(
        platform,
        frontend,
        &mut match_state.sim_runtime,
        process_assets,
        audio,
    )?;
    if frontend
        .loading_session
        .as_ref()
        .and_then(|session| session.native.as_ref())
        .is_some_and(|native| native.variant == NativeLoadingArt::Campaign)
    {
        let session = frontend.loading_session.as_ref().expect("native session");
        let assets = loading_asset_manager(session)
            .ok_or_else(|| anyhow::anyhow!("campaign loading lost its asset manager"))?;
        let mut metadata = crate::rules::campaign_loading::CampaignLoadingMetadata::default();
        let mut names = match_state.campaign_names.clone();
        let csf = process_assets.csf.as_ref().ok_or_else(|| {
            anyhow::anyhow!("campaign loading requires the initialized CSF table")
        })?;
        // Original Full_Init686D46 opens MISSIONMD for the current filename.
        // Missing keys retain SetDefaults' zero/empty values. Missing metadata
        // suppresses DrawLoading552D60 without inventing a generic screen.
        if let Ok(selected) =
            crate::rules::retail_sources::select_ini(assets, assets.ini_roots().mission())
        {
            log::info!(
                "Campaign MISSION source: {}",
                serde_json::json!(selected.source)
            );
            let scenario = session
                .stage
                .startup()
                .campaign()
                .expect("campaign loading owns a campaign startup")
                .scenario();
            metadata.apply_ini(&selected.ini, scenario);
            names.apply_ini(&selected.ini, scenario, csf);
        }
        // Ares ProgressScreenClass_GetLoadTextColor (1002F870, hook
        // 0x642B36) colours the campaign title and briefing with the scheme
        // CacheSP (1002F680, hook 0x686D7F) found, where Get_Side_ColorScheme
        // 0x642B30 picks AlliedLoad/SovietLoad by Campaign.CD; no scheme of
        // that name keeps the stock colour. Only when the Rules are read the
        // Ares way.
        let ares = process_assets.reads_ares_way();
        let ares_text_rgb = if ares {
            crate::app::state::active_rules(frontend, match_state).and_then(|rules| {
                let schemes = &rules.color_schemes;
                scheme_entry_by_name(schemes, metadata.load_text_color())
                    .and_then(|entry| scheme_hsv_by_entry(schemes, entry))
                    .map(|hsv| normalize_rgb(hsv_to_rgb(hsv)))
            })
        } else {
            None
        };
        let presentation = super::campaign_presentation::CampaignLoadingPresentation::new(
            &metadata,
            csf,
            [renderer.gpu.config.width, renderer.gpu.config.height],
            loading_scale(&renderer.gpu),
        );
        let atlas = crate::render::loading_screen_chrome::build_campaign_loading_screen_atlas(
            &renderer.gpu,
            &renderer.batch_renderer,
            assets,
            presentation.width,
            &presentation.background_name,
            &presentation.background_palette_name,
        );
        if atlas.is_none() && !presentation.background_name.is_empty() {
            anyhow::bail!(
                "campaign loading assets unavailable for {}",
                presentation.background_name
            );
        }
        let native = frontend
            .loading_session
            .as_mut()
            .and_then(|session| session.native.as_mut())
            .expect("campaign native session");
        native.campaign_presentation = Some(presentation);
        if let Some(text_rgb) = ares_text_rgb {
            native.text_rgb = text_rgb;
        }
        native.campaign_names = Some(names);
        native.atlas = atlas;
        log::info!("Native campaign loading atlas ready");
        return Ok(());
    }
    let variant = selected_loading_art_variant(frontend);
    ensure_loading_composition_snapshot(&renderer.gpu, frontend, process_assets);
    // DrawLoading553311..5533C0 asks the current mode's Cooperative slot,
    // then selects by ScenarioName rather than the active progress record.
    let cooperative_background = frontend
        .loading_session
        .as_ref()
        .map(|session| session.stage.startup())
        .filter(|startup| {
            startup.launch_session().is_some_and(|launch| {
                launch.mode.class == crate::skirmish_modes::MpModeClass::Cooperative
            })
        })
        .and_then(|startup| {
            frontend
                .multiplayer_shell_runtime
                .cooperative_registry()
                .loading_art(startup.selected_map_file())
        });
    let Some(assets) = frontend
        .loading_session
        .as_ref()
        .and_then(loading_asset_manager)
    else {
        return Err(anyhow::anyhow!(
            "native loading job has no asset manager after initialization"
        ));
    };
    // The atlas is the art the authored viewport uses: a stretched window still
    // loads the 800px set and grows it, exactly as the shell family's pages do.
    let width = LoadingScreenWidth::for_render_width(
        loading_authored_size(
            [renderer.gpu.config.width, renderer.gpu.config.height],
            loading_scale(&renderer.gpu),
        )[0],
    );
    let progress_ramp = frontend
        .loading_session
        .as_ref()
        .and_then(|session| session.native.as_ref())
        .map(|native| native.progress_ramp)
        .ok_or_else(|| anyhow::anyhow!("native loading session lost its progress ramp"))?;
    let prepared_preview = frontend
        .loading_session
        .as_ref()
        .and_then(|session| session.native.as_ref())
        .and_then(|native| native.composition.as_ref())
        .and_then(|composition| composition.preview.as_ref())
        .map(|preview| PreparedLoadingPreviewRgba {
            width: preview.image.width,
            height: preview.image.height,
            rgba: preview.image.rgba.clone(),
        });
    let marker_remaps = frontend
        .loading_session
        .as_ref()
        .and_then(|session| session.native.as_ref())
        .and_then(|native| {
            native
                .composition
                .as_ref()
                .map(|composition| (composition, native.progress_row.skirmish_colors))
        })
        .zip(crate::app::state::active_rules(frontend, match_state))
        .map(|((composition, skirmish_colors), rules)| {
            composition
                .markers
                .iter()
                .map(|marker| {
                    let color_key =
                        skirmish_colors.entry_for_priority(i32::from(marker.color_priority)) as u8;
                    MmpbMarkerRemap {
                        color_key,
                        ramp: *rules.house_color_ramps.ramp(HouseColorIndex(color_key)),
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let composition_input =
        prepared_preview
            .as_ref()
            .map(|preview| LoadingScreenCompositionAtlasInput {
                preview,
                marker_remaps: &marker_remaps,
            });
    let participant_art: Vec<_> = frontend
        .loading_session
        .as_ref()
        .and_then(|session| session.native.as_ref())
        .expect("native loading")
        .progress_row
        .participants
        .iter()
        .map(
            |row| crate::render::loading_screen_chrome::LoadingParticipantArtInput {
                variant: row.variant,
                ramp: row.ramp,
                side_icon: loading_row_insignia(
                    crate::app::state::active_rules(frontend, match_state),
                    row.variant,
                    row.country_index,
                ),
            },
        )
        .collect();
    let country_background = frontend
        .loading_session
        .as_ref()
        .and_then(|session| session.native.as_ref())
        .and_then(|native| native.late_country_loading.as_ref())
        .map(|country| {
            // A template this formatter does not support omits only the
            // background, like missing custom art; it never fails the screen.
            country
                .background_file(width == LoadingScreenWidth::W640)
                .unwrap_or_else(|| {
                    log::warn!("Unsupported Country loading printf template; omitting background");
                    String::new()
                })
        });
    let cooperative_palette = cooperative_background.and_then(|(_, name)| {
        crate::render::loading_screen_chrome::load_named_ui_palette(assets, name)
    });
    let background = cooperative_background.map(|(name, palette_name)| {
        crate::render::loading_screen_chrome::LoadingCountryBackground {
            name,
            palette: cooperative_palette.as_ref(),
            palette_name,
        }
    });
    let background = background.or_else(|| {
        country_background.as_ref().map(|file| {
            let early = frontend
                .loading_session
                .as_ref()
                .and_then(|session| session.native.as_ref())
                .and_then(|native| native.early_country_loading.as_ref())
                .expect("late Country art retains its early palette");
            crate::render::loading_screen_chrome::LoadingCountryBackground {
                name: file,
                palette: early.palette.as_ref(),
                palette_name: &early.palette_name,
            }
        })
    });
    let art = crate::render::loading_screen_chrome::LoadingScreenArtInput {
        variant,
        background,
    };
    let atlas = build_loading_screen_atlas_with_composition(
        &renderer.gpu,
        &renderer.batch_renderer,
        assets,
        art,
        width,
        &progress_ramp,
        composition_input,
        &participant_art,
    );
    if let Some(native) = frontend
        .loading_session
        .as_mut()
        .and_then(|session| session.native.as_mut())
    {
        native.atlas = atlas;
    }
    if frontend
        .loading_session
        .as_ref()
        .and_then(|session| session.native.as_ref())
        .is_some_and(|native| native.atlas.is_some())
    {
        log::info!("Native standard Skirmish loading atlas ready: {variant:?} {width:?}");
        Ok(())
    } else {
        log::warn!("Native standard Skirmish loading atlas failed: {variant:?} {width:?}");
        Err(anyhow::anyhow!(
            "native standard Skirmish loading atlas failed: {variant:?} {width:?}"
        ))
    }
}

pub(crate) fn render_loading_screen(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> LoadingRenderResult {
    let failure_policy = loading_failure_policy(&state.frontend);
    match encode_loading_screen(
        &state.platform,
        &state.renderer,
        &mut state.frontend,
        &mut state.match_state,
        &mut state.process_assets,
        &mut state.audio,
        encoder,
        destination,
    ) {
        Ok(result) => result,
        Err(err) => {
            fail_loading(state, failure_policy, err);
            LoadingRenderResult::Failed
        }
    }
}

fn encode_loading_screen(
    platform: &crate::app::PlatformState,
    renderer: &crate::app::renderer_state::RendererState,
    frontend: &mut crate::app::frontend::state::FrontendState,
    match_state: &mut crate::app::match_runtime::state::MatchState,
    process_assets: &mut crate::app::process_assets::ProcessAssets,
    audio: &mut crate::app::audio_runtime::AppAudioRuntime,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<LoadingRenderResult> {
    if !is_native_loading_session(frontend) {
        return Ok(LoadingRenderResult::GenericFallback);
    }
    if frontend
        .loading_session
        .as_ref()
        .is_some_and(|session| session.next_frame == NextLoadingFrame::Blank)
    {
        render::encode_blank_loading_frame(renderer, encoder, destination);
        return Ok(LoadingRenderResult::NativeRendered);
    }
    ensure_native_loading_atlas(
        platform,
        renderer,
        frontend,
        match_state,
        process_assets,
        audio,
    )?;
    if let Some(session) = frontend.loading_session.as_mut()
        && session.next_frame == NextLoadingFrame::First
        && let Some(native) = session.native.as_mut()
    {
        // Retail composes the LS country surface first, then ProgressClass
        // supplies the first displayed value: selected maps show raw 3, while
        // random-map seed loads halve it to 1.
        native
            .progress
            .advance_progress(native.progress_cadence.effective_percent(3));
    }
    let Some(native) = frontend
        .loading_session
        .as_ref()
        .and_then(|session| session.native.as_ref())
    else {
        return Ok(LoadingRenderResult::GenericFallback);
    };
    if native.suppresses_chrome() {
        render::encode_blank_loading_frame(renderer, encoder, destination);
        return Ok(LoadingRenderResult::NativeRendered);
    }
    render::encode_native_loading_frame(
        renderer,
        native,
        loading_scale(&renderer.gpu),
        encoder,
        destination,
    )?;
    Ok(LoadingRenderResult::NativeRendered)
}

/// Called only after the submitted loading frame and its readbacks. Loading
/// owns acknowledgement, continuation and terminal disposition as one step.
pub(crate) fn after_loading_frame_presented(state: &mut AppState) -> anyhow::Result<()> {
    if !matches!(state.frontend.screen, GameScreen::Loading) {
        return Ok(());
    }
    // The black frame is up: the scenario is read under it on the next frame.
    if let Some(session) = state.frontend.loading_session.as_mut()
        && session.next_frame == NextLoadingFrame::Blank
    {
        session.next_frame = NextLoadingFrame::First;
        state.platform.window.request_redraw();
        return Ok(());
    }
    loading_screen_presented(&mut state.frontend);
    advance_loading(state, true)
}

/// An admitted online loader still reads its map and serves peer progress
/// while the window is covered. Only a presented frame consumes its visual
/// first-frame state, so uncovering a slow load paints it normally.
pub(crate) fn advance_hidden_network_loading(state: &mut AppState) {
    if !network_loading_active(&state.frontend) {
        return;
    }
    if let Err(error) = ensure_native_loading_atlas(
        &state.platform,
        &state.renderer,
        &mut state.frontend,
        &mut state.match_state,
        &mut state.process_assets,
        &mut state.audio,
    )
    .and_then(|()| advance_loading(state, false))
    {
        fail_loading(state, LoadingFailurePolicy::RetryNetworkShell, error);
    }
}

fn advance_loading(state: &mut AppState, show_progress: bool) -> anyhow::Result<()> {
    let policy = loading_failure_policy(&state.frontend);
    match pump_loading(state, show_progress)? {
        LoadingPump::Pending => {
            if show_progress {
                state.platform.window.request_redraw();
            }
        }
        LoadingPump::Finished {
            mut result,
            timed_out,
        } => {
            let disconnected = apply_network_loading_disconnects(
                &mut state.match_state.session,
                &mut result,
                &timed_out,
            );
            log::debug!(target: "vera20k::loading_attempt", "install_begin show_progress={show_progress}");
            super::transitions::finish_fresh_scenario_read(state, *result);
            for destroyed in disconnected {
                crate::app::match_runtime::network_match::post_connection_line(
                    &state.platform,
                    &state.renderer,
                    &state.diag,
                    &state.frontend,
                    &mut state.match_state,
                    &state.process_assets,
                    &mut state.audio.sfx_player,
                    &state.persistence,
                    destroyed,
                );
            }
            log::debug!(target: "vera20k::loading_attempt", "install_end screen={:?} assets_available={} accepted={}", state.frontend.screen, state.process_assets.is_available(), state.match_state.startup.accepted().is_some());
        }
        LoadingPump::Failed(err) => fail_loading(state, policy, err),
    }
    Ok(())
}

/// Native684536 ->5DA750(index, 1) at frame0. Source membership changes
/// independently of the immutable launch and its already constructed House
/// array; the lines post once the scenario is installed.
fn apply_network_loading_disconnects(
    session: &mut crate::app::match_runtime::session::MatchSession,
    result: &mut MapLoadResult,
    indices: &[usize],
) -> Vec<crate::app::match_runtime::network_match::ConnectionDestroyed> {
    use crate::app::match_runtime::network_match::{ConnectionError, destroy_connection};
    // VERA online adaptation: the relay certificate fixes an online match's
    // sources, and the relay departs a player who never becomes ready in the
    // first frame for every client. A local disconnect would leave this
    // client's roster short of that certificate.
    if session.network_return_context()
        == Some(crate::app::match_runtime::session::NetworkReturnContext::Wol)
    {
        return Vec::new();
    }
    let Some(sim) = result.scenario.simulation.as_mut() else {
        // No House array to consult: the connections still drop.
        for &index in indices {
            session.disconnect_network_participant(index);
        }
        return Vec::new();
    };
    indices
        .iter()
        .filter_map(|&index| destroy_connection(session, sim, index, ConnectionError::Lost, None))
        .collect()
}

/// What the shell says after a failed load returned to it, for the policies
/// that go back to the menu or lobby. Native retries the shell without a
/// word (`0x0052E732`, `0x0052E75F`); VERA-only: the player is told why, as
/// the skirmish result card already does.
fn failed_load_notice(policy: LoadingFailurePolicy, err: &anyhow::Error) -> Option<String> {
    matches!(
        policy,
        LoadingFailurePolicy::RetryCampaignShell | LoadingFailurePolicy::RetryNetworkShell
    )
    .then(|| {
        let mut detail = format!("{err:#}");
        if let Some((end, _)) = detail.char_indices().nth(400) {
            detail.truncate(end);
            detail.push('…');
        }
        format!("{}\n\n{detail}", crate::i18n::tr("VERA:LoadingFailed"))
    })
}

fn fail_loading(state: &mut AppState, policy: LoadingFailurePolicy, err: anyhow::Error) {
    fail_loading_with(state, policy, err, true);
}

fn fail_loading_with(
    state: &mut AppState,
    policy: LoadingFailurePolicy,
    err: anyhow::Error,
    notify: bool,
) {
    log::warn!("Could not load map: {err:#}");
    let campaign_room = state.match_state.session.campaign_room().is_some();
    let campaign_replay = state
        .frontend
        .loading_session
        .as_ref()
        .is_some_and(|loading| {
            matches!(
                loading.stage.startup(),
                LoadingStartup::AuthoredCampaignReplay { .. }
            )
        });
    let campaign_notice = if campaign_room {
        Some("VERA:CampaignRoomUnavailable")
    } else if campaign_replay {
        Some("VERA:CampaignReplayUnavailable")
    } else {
        None
    };
    let policy = retire_failed_loading_attempt(
        &mut state.frontend.loading_session,
        &mut state.match_state.startup,
        &mut state.process_assets,
        policy,
    );
    reset_loading_presentation(&mut state.renderer, &mut state.frontend);
    if state.frontend.external_launch {
        state.frontend.external_launch_error = Some(campaign_notice.map_or_else(
            || format!("CnCNet map loading failed: {err:#}"),
            crate::i18n::tr,
        ));
        crate::app::App::return_to_main_menu(state);
        return;
    }
    if matches!(
        policy,
        LoadingFailurePolicy::RetryCampaignShell | LoadingFailurePolicy::RetryNetworkShell
    ) {
        // MainPrepareSession52E718 tests StartScenario's AL; a failed normal
        // campaign start retries the main shell at52E732 ->52D9C9. The
        // network StartScenario call52E745 follows the same retry at52E75F;
        // GameMode3 re-enters LAN at52DAEF/52E356, so retain its lobby context.
        crate::app::App::return_to_main_menu(state);
        if notify
            && let Some(text) = campaign_notice
                .map(crate::i18n::tr)
                .or_else(|| failed_load_notice(policy, &err))
        {
            crate::app::App::open_shell_notice(
                &state.platform.window,
                &state.renderer,
                &mut state.frontend,
                &state.process_assets,
                &state.persistence.options_profile,
                text,
            );
        }
    } else if policy == LoadingFailurePolicy::ReportNativeFailure {
        state.frontend.screen = GameScreen::MissionResult {
            title: crate::i18n::tr("VERA:LoadingFailed"),
            detail: campaign_notice.map_or_else(|| format!("{err:#}"), crate::i18n::tr),
        };
    } else {
        super::transitions::apply_map_load_result(
            state,
            super::transitions::fallback_map_load_result(),
        );
    }
}

/// Which loading frame the next native present draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NextLoadingFrame {
    /// The black frame that replaces the closed shell.
    Blank,
    /// The loading screen's first frame (3%), after the scenario is read.
    First,
    /// A later loading frame.
    Later,
}

pub(crate) fn next_native_loading_frame(
    frontend: &crate::app::frontend::state::FrontendState,
) -> Option<NextLoadingFrame> {
    let session = frontend.loading_session.as_ref()?;
    session.native.as_ref()?;
    Some(session.next_frame)
}

/// Show the static F0 saved-load progress dialog before synchronous resource
/// preparation. Original559D60 invokes623230 with TXT_LOADING and no Cancel
/// caption. This is independent of fresh scenario artwork, milestones or audio.
/// The retained B7 browser is not painted or mutated, so failure can redraw it.
/// The dialog paints no pointer; the window's own stays as the frame owner
/// last set it, hidden wherever mouse.sha loaded.
pub(crate) fn present_saved_game_loading(
    platform: &mut crate::app::PlatformState,
    renderer: &mut crate::app::renderer_state::RendererState,
    frontend: &mut crate::app::frontend::state::FrontendState,
    match_state: &mut crate::app::match_runtime::state::MatchState,
    process_assets: &crate::app::process_assets::ProcessAssets,
    persistence: &crate::app::persistence::PersistenceState,
) -> anyhow::Result<bool> {
    if !crate::app::App::ensure_skirmish_shell_chrome(
        renderer,
        frontend,
        process_assets,
        &persistence.options_profile,
    ) {
        anyhow::bail!("Saved-game loading dialog has no shell chrome");
    }
    let Some(output) = renderer
        .gpu
        .acquire_frame()
        .map_err(|error| anyhow::anyhow!("saved-game loading surface: {error}"))?
    else {
        return Ok(false);
    };
    let mut encoder = renderer
        .gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Saved Game Loading"),
        });
    renderer.window_ui.begin_frame();
    crate::app::frontend::skirmish_shell_render::render_saved_game_loading(
        platform,
        renderer,
        frontend,
        match_state,
        process_assets,
        persistence,
        &mut encoder,
        &output.texture,
    )?;
    renderer.performance_overlay.encode(
        &renderer.gpu,
        &renderer.batch_renderer,
        &mut encoder,
        &output.texture,
        &renderer.bit_font,
        None,
    );
    renderer
        .gpu
        .encode_color_vision(&mut encoder, &output.texture);
    renderer.gpu.queue.submit(std::iter::once(encoder.finish()));
    output.present(&renderer.gpu.queue);
    Ok(true)
}

/// After the load gamemd switches to the game mode and shows black until
/// the first game frame (`0x00683E07..0x00683E1C`, again at
/// `0x0052EAA1..0x0052EAEB`); the tactical install runs under it.
pub(crate) fn present_game_mode_blank(
    platform: &crate::app::PlatformState,
    renderer: &crate::app::renderer_state::RendererState,
    match_state: &crate::app::match_runtime::state::MatchState,
) {
    if crate::app::App::online_start_runs_while_hidden(platform, match_state) {
        return;
    }
    if let Err(err) = render::present_blank(
        &renderer.gpu,
        &renderer.shell_surface_presenter,
        &renderer.batch_renderer,
        &renderer.performance_overlay,
        &renderer.bit_font,
    ) {
        log::warn!("Game-mode blank frame failed: {err:#}");
    }
}

fn loading_screen_presented(frontend: &mut crate::app::frontend::state::FrontendState) {
    let Some(session) = frontend.loading_session.as_mut() else {
        frontend.loading_progress.advance_progress(3);
        return;
    };
    session.next_frame = NextLoadingFrame::Later;
    log::debug!(target: "vera20k::loading_attempt", "frame_presented native_progress={:?}", session.native.as_ref().map(|n| n.progress.current_value()));
}

fn selected_loading_art_variant(
    frontend: &crate::app::frontend::state::FrontendState,
) -> Option<LoadingArtVariant> {
    if !matches!(frontend.screen, GameScreen::Loading) {
        return None;
    }
    frontend
        .loading_session
        .as_ref()
        .and_then(|session| session.native.as_ref())
        .and_then(|native| match native.variant {
            NativeLoadingArt::StockOffline(variant) => variant,
            NativeLoadingArt::Campaign => None,
        })
}

pub(super) fn loading_art_variant_from_launch_country(
    country: LaunchCountry,
) -> Option<LoadingArtVariant> {
    // DrawLoading553412..5534CE and insignia4E3560 define only this stock
    // presentation domain. Preserve the wider HouseType registry identity.
    Some(match country.raw_index() {
        0 => LoadingArtVariant::Americans,
        1 => LoadingArtVariant::Alliance,
        2 => LoadingArtVariant::French,
        3 => LoadingArtVariant::Germans,
        4 => LoadingArtVariant::British,
        5 => LoadingArtVariant::Africans,
        6 => LoadingArtVariant::Arabs,
        7 => LoadingArtVariant::Confederation,
        8 => LoadingArtVariant::Russians,
        9 => LoadingArtVariant::Yuri,
        _ => return None,
    })
}

/// `Game_GetFlagSurface @ 0x1001DCE0` (Ares patch site `0x4E3562`, 5 bytes)
/// answers each loading row's insignia from that row's own HouseType index, so
/// an added Country draws its `File.Flag` where the stock chain at `0x4E3579`
/// has no entry. An observer row is not a Country index and keeps the stock
/// `-3` insignia; a game without Ares (`RuleSet::ares`) answers `None` and
/// keeps the table.
fn loading_row_insignia(
    rules: Option<&crate::rules::ruleset::RuleSet>,
    variant: Option<LoadingArtVariant>,
    country_index: u16,
) -> Option<String> {
    if variant == Some(LoadingArtVariant::Observer) {
        return None;
    }
    rules?.country_insignia_pcx(country_index)
}

/// Normalize an 8-bit RGB triple to 0..1.
fn normalize_rgb(rgb: [u8; 3]) -> [f32; 3] {
    [
        rgb[0] as f32 / 255.0,
        rgb[1] as f32 / 255.0,
        rgb[2] as f32 / 255.0,
    ]
}

/// The mapping the loading screen composes and paints in: gamemd's centered,
/// 1:1 art viewport, or that same authored dialog scaled to fit the window.
fn loading_scale(gpu: &GpuContext) -> ShellScale {
    loading_screen_scale(
        crate::app::frontend::shell_layout::fills_window(gpu),
        [gpu.config.width, gpu.config.height],
    )
}

fn advance_and_present_native_progress<'a>(
    show_progress: bool,
    gpu: &'a GpuContext,
    presenter: &'a ShellSurfacePresenter,
    depth_view: &'a wgpu::TextureView,
    batch: &'a BatchRenderer,
    font: &'a BitFont,
    performance_overlay: &'a crate::app::diagnostics::performance_overlay::PerformanceOverlay,
    native: &'a mut NativeLoadingScreenState,
    raw_percent: u32,
    render_size: [u32; 2],
    scale: ShellScale,
    network: Option<crate::app::lan_service::LoadingNetworkService<'a>>,
) -> anyhow::Result<()> {
    let mut selected = LoadingPhaseProgress::select(
        Some(native),
        false,
        show_progress,
        network,
        |native, network| {
            RenderingProgressSink::new(
                native,
                network,
                gpu,
                presenter,
                depth_view,
                batch,
                font,
                performance_overlay,
                render_size,
                scale,
            )
        },
    );
    selected.sink.milestone(raw_percent)
}

/// Sink that synchronously re-renders and presents the loading screen on each
/// advancing milestone, implementing gamemd's per-milestone visible handoff
/// through wgpu. Render or surface-acquire failures are logged and swallowed so
/// they never abort the map load.
struct RenderingProgressSink<'a> {
    gpu: &'a GpuContext,
    presenter: &'a ShellSurfacePresenter,
    depth_view: &'a wgpu::TextureView,
    batch: &'a BatchRenderer,
    font: &'a BitFont,
    performance_overlay: &'a crate::app::diagnostics::performance_overlay::PerformanceOverlay,
    gate: GatedProgressSink<'a>,
    atlas: &'a LoadingScreenAtlas,
    composition: Option<&'a LoadingCompositionSnapshot>,
    campaign_presentation: Option<&'a CampaignLoadingPresentation>,
    backing_rgb: [f32; 3],
    text_rgb: [f32; 3],
    render_size: [u32; 2],
    scale: ShellScale,
}

impl<'a> RenderingProgressSink<'a> {
    fn new(
        native: &'a mut NativeLoadingScreenState,
        network: Option<crate::app::lan_service::LoadingNetworkService<'a>>,
        gpu: &'a GpuContext,
        presenter: &'a ShellSurfacePresenter,
        depth_view: &'a wgpu::TextureView,
        batch: &'a BatchRenderer,
        font: &'a BitFont,
        performance_overlay: &'a crate::app::diagnostics::performance_overlay::PerformanceOverlay,
        render_size: [u32; 2],
        scale: ShellScale,
    ) -> Self {
        Self {
            gpu,
            presenter,
            depth_view,
            batch,
            font,
            performance_overlay,
            gate: GatedProgressSink {
                progress: &mut native.progress,
                progress_row: &mut native.progress_row,
                cadence: native.progress_cadence,
                network,
            },
            atlas: native
                .atlas
                .as_ref()
                .expect("selected rendering sink has atlas"),
            composition: native.composition.as_ref(),
            campaign_presentation: native.campaign_presentation.as_ref(),
            backing_rgb: native.backing_rgb,
            text_rgb: native.text_rgb,
            render_size,
            scale,
        }
    }
}

impl LoadingProgressSink for RenderingProgressSink<'_> {
    fn milestone(&mut self, raw_percent: u32) -> anyhow::Result<()> {
        if self.gate.advance(raw_percent) {
            let effective_percent = self.gate.progress.current_percent();
            log::debug!(target: "vera20k::loading_attempt", "progress_present raw={raw_percent}");
            if let Err(err) = render::present_native_loading(
                self.gpu,
                self.presenter,
                self.depth_view,
                self.batch,
                self.font,
                self.performance_overlay,
                self.atlas,
                self.composition,
                self.campaign_presentation,
                self.gate.progress_row,
                self.gate.progress,
                self.backing_rgb,
                self.text_rgb,
                self.render_size,
                self.scale,
            ) {
                log::warn!(
                    "Native loading repaint at raw milestone {raw_percent} \
                     (effective {effective_percent}) failed: {err:#}"
                );
            }
        }
        self.gate.service()
    }
}

fn gamemd_ftol_positive_domain(value: f64) -> i32 {
    debug_assert!(value >= 0.0);
    let nearest = value.round();
    if (value - nearest).abs() <= FTOL_EPSILON {
        return nearest as i32;
    }

    // Exact fractional x87 control-word behavior remains a narrow follow-up.
    value as i32
}
