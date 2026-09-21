//! Loading attempt, asset lease, phase progress and presentation handoff.
//!
//! This module sits above simulation and owns loading-screen progress behavior
//! verified from gamemd.exe. It also owns the request/session boundary used by
//! app loop. The prepared map, prefix and startup move together; remaining
//! loading stays synchronous and retains the native progress/presentation order.
//! Private `render` owns frame drawing; this module retains preparation, cadence,
//! failure and after-present continuation policy. Request/phase regressions live
//! in the private `tests` child.

mod render;

#[cfg(test)]
mod tests;

use crate::app::AppState;
use crate::app::loading::composition::{
    LoadingCompositionSnapshot, LoadingParticipantId, LoadingStartAssignment,
    RANDOM_MAP_PREVIEW_FILE, build_loading_composition, build_random_map_loading_composition,
};
use crate::app::loading::fresh_scenario::FreshScenarioLoadContextDescriptor;
use crate::app::loading::init::{self, MapLoadInitial, MapLoadResult};
use crate::app::loading::progress_row::LoadingProgressRowSnapshot;
use crate::assets::asset_manager::AssetManager;
use crate::assets::pal_file::Color;
use crate::assets::pcx_file::PcxFile;
use crate::map::preview::DecodedPreview;
use crate::match_bootstrap::{LoadingStartup, PreparedMatchStartup};
use crate::render::batch::BatchRenderer;
use crate::render::bit_font::BitFont;
use crate::render::gpu::GpuContext;
use crate::render::loading_screen_chrome::{
    LoadingArtVariant, LoadingScreenAtlas, LoadingScreenCompositionAtlasInput,
    LoadingScreenWidth, MmpbMarkerRemap, PreparedLoadingPreviewRgba,
    build_loading_screen_atlas_with_composition,
};
use crate::render::shell_surface_present::ShellSurfacePresenter;
use crate::rules::color_scheme::{
    ColorSchemeEntry, hsv_to_rgb, scheme_entry_by_name, scheme_entry_for_priority,
    scheme_hsv_by_entry,
};
use crate::rules::house_colors::{HouseColorIndex, HouseColorRamps};
use crate::sim::scenario_bootstrap::StockOfflinePrefixProjection;
use crate::skirmish_launch::{LaunchCountry, SkirmishLaunchSession};
use crate::ui::game_screen::GameScreen;
use crate::ui::main_menu::SkirmishSettings;
use std::path::{Path, PathBuf};

const STANDARD_SKIRMISH_PROGRESS_MAX: f64 = 100.0;
const PROGRESS_PERCENT_SCALE: f64 = 0.01;
const PERCENT_DISPLAY_SCALE: f64 = 100.0;
const FTOL_EPSILON: f64 = 0.000_001;
/// Static loading colors used only when rules `[Colors]` are unavailable
/// (missing assets / headless); replaced by `resolve_player_colors` once rules
/// load.
const FALLBACK_BACKING_RGB: [f32; 3] = [0.22, 0.22, 0.22];
const FALLBACK_PROGRESS_RAMP: [Color; 16] = [Color::rgb(180, 180, 180); 16];
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

    #[cfg(test)]
    pub fn max_value(&self) -> f64 {
        self.max_value
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

    fn set_percent(&mut self, percent: f64) -> bool {
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
    fn milestone(&mut self, percent: u32);
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
    /// from `RandMap.img`, but Full Init has already regenerated the `.SED`,
    /// copied accepted start staging, and run both selected-mode callbacks
    /// before DrawLoadingScreen. Both branches therefore swallow the loader's
    /// raw 8 here and hand it to the first post-frame pump.
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
    cadence: NativeLoadingProgressCadence,
}

impl LoadingProgressSink for GatedProgressSink<'_> {
    fn milestone(&mut self, raw_percent: u32) {
        self.progress
            .advance_progress(self.cadence.effective_percent(raw_percent));
    }
}

/// Sink for the generic (non-native) map load, which has no progress bar.
struct NoopProgressSink;

impl LoadingProgressSink for NoopProgressSink {
    fn milestone(&mut self, _percent: u32) {}
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
    fn milestone(&mut self, raw_percent: u32) {
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
        rendering: impl FnOnce(&'a mut NativeLoadingScreenState) -> RenderingProgressSink<'a>,
    ) -> Self {
        let Some(native) = native else {
            return Self {
                sink: SelectedProgressSink::Generic(NoopProgressSink),
                native_theater_cache_mismatch: false,
                runtime_color_scheme_count: 0,
            };
        };
        let runtime_color_scheme_count = native.runtime_color_scheme_count;
        let sink = if native.atlas.is_some() {
            SelectedProgressSink::Rendering(rendering(native))
        } else {
            SelectedProgressSink::Gated(GatedProgressSink {
                cadence: native.progress_cadence,
                progress: &mut native.progress,
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
    fallback_skirmish_settings: SkirmishSettings,
}

impl LoadingRequest {
    pub(crate) fn accepted_skirmish(
        startup: PreparedMatchStartup,
        fallback_skirmish_settings: SkirmishSettings,
    ) -> Self {
        Self {
            startup: LoadingStartup::Accepted(startup),
            accepted_rmg_start_staging: None,
            random_map_preview: None,
            fallback_skirmish_settings,
        }
    }

    pub(crate) fn unverified_legacy_skirmish(
        skirmish_launch_session: SkirmishLaunchSession,
        seed: crate::match_bootstrap::MatchSeed,
        fallback_skirmish_settings: SkirmishSettings,
    ) -> Self {
        Self {
            startup: LoadingStartup::UnverifiedLegacy {
                session: skirmish_launch_session,
                seed,
            },
            accepted_rmg_start_staging: None,
            random_map_preview: None,
            fallback_skirmish_settings,
        }
    }

    pub(crate) fn generic_map_load(
        selected_map_file: impl Into<String>,
        fallback_skirmish_settings: SkirmishSettings,
    ) -> Self {
        Self {
            startup: LoadingStartup::Generic {
                selected_map_file: selected_map_file.into(),
            },
            accepted_rmg_start_staging: None,
            random_map_preview: None,
            fallback_skirmish_settings,
        }
    }

    pub(crate) fn selected_map_file(&self) -> &str {
        self.startup().selected_map_file()
    }

    /// Run the request's authoritative initial-map entry. The retained random
    /// preview is intentionally not an argument: active retail's `.SED` reader
    /// regenerates gameplay after Start, while the preview remains a loading-
    /// composition fallback only.
    fn load_initial_with_assets(
        &self,
        ra2_dir: std::path::PathBuf,
        asset_manager: &mut AssetManager,
        progress: &mut dyn LoadingProgressSink,
    ) -> anyhow::Result<MapLoadInitial> {
        init::load_map_initial_with_assets(
            ra2_dir,
            asset_manager,
            Some(self.selected_map_file()),
            progress,
        )
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

    #[cfg(test)]
    fn random_map_preview(&self) -> Option<&crate::map::rmg::GeneratedMap> {
        self.random_map_preview.as_ref()
    }

    fn admit_context(
        &mut self,
        initial: &MapLoadInitial,
    ) -> anyhow::Result<FreshScenarioLoadContextDescriptor> {
        FreshScenarioLoadContextDescriptor::admit_stock_offline(
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
        progress: &mut dyn LoadingProgressSink,
    ) -> anyhow::Result<PreparedScenarioLoad> {
        let initial = self.load_initial_with_assets(ra2_dir, assets, progress)?;
        self.prepare_initial(initial)
    }

    #[cfg(test)]
    pub(crate) fn load_random_map_snapshot_for_test(
        self,
        ra2_dir: PathBuf,
        assets: &mut AssetManager,
        progress: &mut dyn LoadingProgressSink,
    ) -> anyhow::Result<init::RandomMapLaunchSnapshot> {
        let prepared = self.prepare(ra2_dir, assets, progress)?;
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

    fn prepare_initial(mut self, initial: MapLoadInitial) -> anyhow::Result<PreparedScenarioLoad> {
        let context = self.admit_context(&initial)?;
        Ok(PreparedScenarioLoad {
            request: self,
            initial,
            context,
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
}

enum LoadingStage {
    Selected(LoadingRequest),
    Prepared(PreparedScenarioLoad),
}

impl LoadingStage {
    fn request(&self) -> &LoadingRequest {
        match self {
            Self::Selected(request) => request,
            Self::Prepared(prepared) => &prepared.request,
        }
    }
}

pub(crate) struct NativeLoadingScreenState {
    pub variant: LoadingArtVariant,
    local_side_index: u8,
    /// Local player's MP color scheme — source of the G3 solid backing fill and
    /// bar remap. Derived from the launch session, not the country variant.
    pub color_index: HouseColorIndex,
    /// Empty-bar backing fill color (normalized RGB), resolved from the player's
    /// `[Colors]` scheme HSV. Falls back to a static shade when `[Colors]` is
    /// unavailable (e.g. assets missing).
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
    /// Native constructs two runtime ColorScheme objects per current `[Colors]`
    /// entry. Capture that pre-load count before the later rules reset.
    runtime_color_scheme_count: usize,
    progress_cadence: NativeLoadingProgressCadence,
}

impl NativeLoadingScreenState {
    fn standard_skirmish(
        variant: LoadingArtVariant,
        local_side_index: u8,
        color_index: HouseColorIndex,
        progress_row: LoadingProgressRowSnapshot,
        progress_cadence: NativeLoadingProgressCadence,
    ) -> Self {
        Self {
            variant,
            local_side_index,
            color_index,
            // Static placeholders; replaced by `resolve_player_colors` once rules load.
            backing_rgb: FALLBACK_BACKING_RGB,
            text_rgb: [1.0, 1.0, 1.0],
            progress_ramp: FALLBACK_PROGRESS_RAMP,
            progress: LoadingProgressState::standard_skirmish(),
            progress_row,
            atlas: None,
            composition: None,
            runtime_color_scheme_count: 0,
            progress_cadence,
        }
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
        let text_scheme = if self.local_side_index == 0 {
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
        self.progress_ramp = *ramps.ramp(self.color_index);
    }
}

pub(crate) struct LoadingSession {
    stage: LoadingStage,
    native: Option<NativeLoadingScreenState>,
    job: LoadingJob,
    first_frame_presented: bool,
}

impl LoadingSession {
    fn native_pump_blocked(&self) -> bool {
        self.native
            .as_ref()
            .is_some_and(|native| native.atlas.is_none())
    }

    fn from_request(request: LoadingRequest) -> Self {
        let native = match request.skirmish_launch_session() {
            Some(skirmish_launch_session) => {
                let variant =
                    loading_art_variant_from_launch_country(skirmish_launch_session.local.country);
                // `local.color_index` is the gamemd color priority; resolve to a
                // `[Colors]` entry index (priority LUT + /2).
                let color_index = HouseColorIndex(scheme_entry_for_priority(
                    skirmish_launch_session.local.color_index as i32,
                ) as u8);
                let progress_cadence = NativeLoadingProgressCadence::for_selected_map_file(
                    request.selected_map_file(),
                );
                Some(NativeLoadingScreenState::standard_skirmish(
                    variant,
                    skirmish_launch_session.local.country.side_index(),
                    color_index,
                    LoadingProgressRowSnapshot::from_launch_session(skirmish_launch_session),
                    progress_cadence,
                ))
            }
            None => None,
        };
        Self {
            stage: LoadingStage::Selected(request),
            native,
            job: LoadingJob::new(),
            first_frame_presented: false,
        }
    }
}

enum LoadingPump {
    Pending,
    Finished(MapLoadResult),
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

pub(crate) fn begin_loading(state: &mut AppState, request: LoadingRequest) {
    let mut session = LoadingSession::from_request(request);
    session.job.ra2_dir = state
        .platform
        .game_config
        .as_ref()
        .map(|config| config.paths.ra2_dir.clone());
    // Resolve the backing fill from the live rules `[Colors]` schemes now that
    // `state.rules` is reachable (the native ctor only sees the launch session).
    if let (Some(native), Some(rules)) = (session.native.as_mut(), state.rules()) {
        native.resolve_player_colors(&rules.color_schemes, &rules.house_color_ramps);
    }
    let now_ms = crate::app::match_runtime::sim_tick::monotonic_frame_pacer_ms(
        state,
        std::time::Instant::now(),
    );
    reset_loading_presentation(state);
    replace_loading_attempt(
        &mut state.frontend.loading_session,
        &mut state.match_state.startup,
        &mut state.process_assets,
        &mut state.audio,
        session,
        now_ms,
    );
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
    let correlation = next
        .stage
        .request()
        .startup()
        .accepted()
        .map(|s| s.correlation);
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
    if let Some(assets) = audio_service_asset_manager(process_assets, Some(&session)) {
        let _ = audio.play_theme("LOADING", assets, now_ms);
    }
    session
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

pub(crate) fn loading_map_name(state: &AppState) -> Option<&str> {
    state
        .frontend
        .loading_session
        .as_ref()
        .map(|session| session.stage.request().selected_map_file())
}

pub(crate) fn clear_loading_state(state: &mut AppState) {
    retire_loading_attempt(
        &mut state.frontend.loading_session,
        &mut state.process_assets,
    );
    reset_loading_presentation(state);
}

fn reset_loading_presentation(state: &mut AppState) {
    state.frontend.loading_screen_atlas = None;
    state.frontend.loading_progress = LoadingProgressState::standard_skirmish();
}

fn retire_loading_attempt(
    slot: &mut Option<LoadingSession>,
    assets: &mut crate::app::process_assets::ProcessAssets,
) {
    if let Some(session) = slot.take() {
        session.job.retire(assets);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LoadingFailurePolicy {
    ReportNativeFailure,
    InstallGenericFallback,
}

fn retire_failed_loading_attempt(
    slot: &mut Option<LoadingSession>,
    startup: &mut crate::app::match_runtime::startup::MatchStartup,
    assets: &mut crate::app::process_assets::ProcessAssets,
    policy: LoadingFailurePolicy,
) -> LoadingFailurePolicy {
    retire_loading_attempt(slot, assets);
    if policy == LoadingFailurePolicy::ReportNativeFailure {
        startup.clear();
    }
    policy
}

/// The handle the player launched this skirmish under, as shown on the loading
/// screen's progress row. `None` outside a skirmish launch.
pub(crate) fn launch_player_name(state: &AppState) -> Option<String> {
    state
        .frontend
        .loading_session
        .as_ref()
        .and_then(|session| session.stage.request().skirmish_launch_session())
        .map(|launch| launch.player_name.clone())
}

fn is_native_loading_session(state: &AppState) -> bool {
    state
        .frontend
        .loading_session
        .as_ref()
        .is_some_and(|session| session.native.is_some())
}

fn pump_loading_after_present(state: &mut AppState) -> LoadingPump {
    let Some(session) = state.frontend.loading_session.take() else {
        return LoadingPump::Pending;
    };
    if session.native_pump_blocked() {
        session.job.retire(&mut state.process_assets);
        return LoadingPump::Failed(anyhow::anyhow!(
            "native Skirmish loading renderer was not ready before the first loading pump"
        ));
    }

    if matches!(session.stage, LoadingStage::Selected(_)) {
        return match prepare_loading_session(
            &mut state.process_assets,
            session,
            false,
            state
                .platform
                .game_config
                .as_ref()
                .map(|c| c.paths.ra2_dir.clone()),
        ) {
            Ok(session) => {
                state.frontend.loading_session = Some(session);
                LoadingPump::Pending
            }
            Err(err) => LoadingPump::Failed(err),
        };
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
    } = prepared;
    log::debug!(target: "vera20k::loading_attempt", "remaining_load_begin");
    let result = (|| -> anyhow::Result<MapLoadResult> {
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
        // split-borrows (gpu/depth_view/batch shared, vxl_compute &mut,
        // native.progress &mut, native.atlas shared, request shared) all
        // hold simultaneously.
        let render_size = [
            state.renderer.gpu.config.width,
            state.renderer.gpu.config.height,
        ];
        // The pre-parse swallowed the loader's raw 8 so it could not present
        // before the first frame; hand it over now for either native cadence.
        if let Some(native) = native.as_mut()
            && native
                .progress_cadence
                .prepares_scenario_before_first_frame()
        {
            advance_and_present_native_progress(
                &state.renderer.gpu,
                &state.renderer.shell_surface_presenter,
                &state.renderer.depth_view,
                &state.renderer.batch_renderer,
                &state.renderer.bit_font,
                native,
                8,
                render_size,
            );
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
        let (native_rules_owner, tile_variant_selector_cache) =
            state.process_assets.native_rules_mut_with_tile_cache();
        let native_rules_owner =
            native_rules_owner.expect("native Rules availability checked before split borrow");
        let load_result = {
            let mut progress = LoadingPhaseProgress::select(
                native.as_mut(),
                native_theater_cache_mismatch,
                |native| {
                    let backing_rgb = native.backing_rgb;
                    let text_rgb = native.text_rgb;
                    let cadence = native.progress_cadence;
                    let atlas = native
                        .atlas
                        .as_ref()
                        .expect("selected rendering sink has atlas");
                    let composition = native.composition.as_ref();
                    RenderingProgressSink {
                        gpu: &state.renderer.gpu,
                        presenter: &state.renderer.shell_surface_presenter,
                        depth_view: &state.renderer.depth_view,
                        batch: &state.renderer.batch_renderer,
                        font: &state.renderer.bit_font,
                        progress: &mut native.progress,
                        progress_row: &native.progress_row,
                        atlas,
                        composition,
                        backing_rgb,
                        text_rgb,
                        render_size,
                        cadence,
                    }
                },
            );
            init::load_map_from_initial(
                &state.renderer.gpu,
                &state.renderer.batch_renderer,
                asset_manager,
                initial,
                startup,
                fresh_scenario_context,
                &request.fallback_skirmish_settings,
                progress.native_theater_cache_mismatch,
                progress.runtime_color_scheme_count,
                state.renderer.vxl_compute.as_mut(),
                native_rules_owner,
                shared_cell_dummy,
                tile_variant_selector_cache,
                &mut progress.sink,
            )
        };

        match load_result {
            Ok(mut result) => {
                if let Some(native) = native.as_mut() {
                    let terminal_raw_percent = native.progress_cadence.terminal_raw_percent();
                    advance_and_present_native_progress(
                        &state.renderer.gpu,
                        &state.renderer.shell_surface_presenter,
                        &state.renderer.depth_view,
                        &state.renderer.batch_renderer,
                        &state.renderer.bit_font,
                        native,
                        terminal_raw_percent,
                        render_size,
                    );
                }
                result.asset_manager = job.asset_manager.take();
                Ok(result)
            }
            Err(err) => Err(err),
        }
    })();
    match result {
        Ok(result) => LoadingPump::Finished(result),
        Err(err) => {
            job.retire(&mut state.process_assets);
            LoadingPump::Failed(err)
        }
    }
}

fn ensure_job_asset_manager(state: &mut AppState) -> anyhow::Result<()> {
    let Some(mut session) = state.frontend.loading_session.take() else {
        return Ok(());
    };
    let result = ensure_session_job_asset_manager(
        &mut state.process_assets,
        &mut session,
        state
            .platform
            .game_config
            .as_ref()
            .map(|c| c.paths.ra2_dir.clone()),
    );
    state.frontend.loading_session = Some(session);
    result
}

fn loading_asset_manager(session: &LoadingSession) -> Option<&AssetManager> {
    session.job.asset_manager.as_ref()
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
            AssetManager::new(
                session
                    .job
                    .ra2_dir
                    .as_deref()
                    .expect("RA2 directory was initialized above"),
            )?
        };
        session.job.asset_manager = Some(asset_manager);
    }
    Ok(())
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
    mut session: LoadingSession,
    before_first_frame: bool,
    configured_ra2_dir: Option<PathBuf>,
) -> anyhow::Result<LoadingSession> {
    if let Err(err) =
        ensure_session_job_asset_manager(process_assets, &mut session, configured_ra2_dir)
    {
        session.job.retire(process_assets);
        return Err(err);
    }
    let LoadingSession {
        stage,
        mut native,
        mut job,
        first_frame_presented,
    } = session;
    let stage = match stage {
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
            let prepared = if !before_first_frame && let Some(native) = native.as_mut() {
                let mut sink = GatedProgressSink {
                    progress: &mut native.progress,
                    cadence: native.progress_cadence,
                };
                request.prepare(ra2_dir, assets, &mut sink)
            } else {
                request.prepare(ra2_dir, assets, &mut NoopProgressSink)
            };
            match prepared {
                Ok(prepared) => LoadingStage::Prepared(prepared),
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
        first_frame_presented,
    })
}

/// Resolve native Scenario inputs before constructing the first loading frame.
///
/// Full Init runs both selected-mode callbacks before DrawLoadingScreen. Fixed
/// maps additionally derive their preview from the parsed scenario; random maps
/// use `RandMap.img` pixels but still require regenerated header data and the
/// accepted staged starts. The loader's raw 8 is intentionally swallowed here
/// and visibly handed off only after the first frame has been presented.
fn prepare_scenario_initial_before_first_frame(state: &mut AppState) -> anyhow::Result<()> {
    let should_prepare = state
        .frontend
        .loading_session
        .as_ref()
        .and_then(|session| session.native.as_ref())
        .is_some_and(|native| {
            native
                .progress_cadence
                .prepares_scenario_before_first_frame()
        })
        && state
            .frontend
            .loading_session
            .as_ref()
            .is_some_and(|session| matches!(session.stage, LoadingStage::Selected(_)));
    if !should_prepare {
        return Ok(());
    }

    let Some(session) = state.frontend.loading_session.take() else {
        return Ok(());
    };
    state.frontend.loading_session = Some(prepare_loading_session(
        &mut state.process_assets,
        session,
        true,
        state
            .platform
            .game_config
            .as_ref()
            .map(|c| c.paths.ra2_dir.clone()),
    )?);
    Ok(())
}

/// Decode the random-map preview bitmap written by the random-map setup dialog.
///
/// This is the whole preview source for a random-map load: gamemd reads the same
/// file rather than deriving a preview from the scenario. A missing or unreadable
/// file omits the preview layer; every other layer still draws.
fn decode_random_map_loading_preview(ra2_dir: &Path) -> Option<DecodedPreview> {
    let path = ra2_dir.join(RANDOM_MAP_PREVIEW_FILE);
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
            let (participant, color_priority) = if participant_index == 0 {
                (
                    LoadingParticipantId::Local,
                    launch_session.local.color_index,
                )
            } else {
                let opponent_index = participant_index - 1;
                let opponent = launch_session.opponents.get(opponent_index)?;
                (
                    LoadingParticipantId::Opponent(opponent_index),
                    opponent.color_index,
                )
            };
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
fn ensure_loading_composition_snapshot(state: &mut AppState) {
    let snapshot = {
        let Some(session) = state.frontend.loading_session.as_ref() else {
            return;
        };
        let Some(native) = session.native.as_ref() else {
            return;
        };
        if native.composition.is_some() {
            return;
        }
        let LoadingStage::Prepared(prepared) = &session.stage else {
            return;
        };
        let context = &prepared.context;
        let initial = &prepared.initial;
        let launch_session = context.stock_offline_launch().session();
        let projection = context.stock_offline_projection();
        let render_size = [
            state.renderer.gpu.config.width,
            state.renderer.gpu.config.height,
        ];
        match native.progress_cadence {
            NativeLoadingProgressCadence::SelectedMap => {
                let assignments = selected_map_start_assignments(launch_session, Some(projection));
                build_loading_composition(
                    initial.map_data(),
                    launch_session,
                    state.process_assets.csf.as_ref(),
                    render_size,
                    &assignments,
                )
            }
            NativeLoadingProgressCadence::RandomMapHalved => {
                let preview = session
                    .job
                    .ra2_dir
                    .as_deref()
                    .and_then(decode_random_map_loading_preview);
                let assignments = selected_map_start_assignments(launch_session, Some(projection));
                build_random_map_loading_composition(
                    launch_session,
                    state.process_assets.csf.as_ref(),
                    render_size,
                    preview,
                    initial.map_data(),
                    projection.active_scenario_waypoints(),
                    &assignments,
                )
            }
        }
    };

    if let Some(native) = state
        .frontend
        .loading_session
        .as_mut()
        .and_then(|session| session.native.as_mut())
    {
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

pub(crate) fn ensure_native_loading_atlas(state: &mut AppState) -> anyhow::Result<()> {
    let Some(variant) = selected_loading_art_variant(state) else {
        return Ok(());
    };
    if state
        .frontend
        .loading_session
        .as_ref()
        .and_then(|session| session.native.as_ref())
        .and_then(|native| native.atlas.as_ref())
        .is_some()
    {
        return Ok(());
    }
    if state
        .frontend
        .loading_session
        .as_ref()
        .and_then(loading_asset_manager)
        .is_none()
    {
        ensure_job_asset_manager(state)?;
    }
    let loading_archives_ready = state
        .frontend
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
    prepare_scenario_initial_before_first_frame(state)?;
    ensure_loading_composition_snapshot(state);
    let Some(assets) = state
        .frontend
        .loading_session
        .as_ref()
        .and_then(loading_asset_manager)
    else {
        return Err(anyhow::anyhow!(
            "native loading job has no asset manager after initialization"
        ));
    };
    let width = LoadingScreenWidth::for_render_width(state.renderer.gpu.config.width);
    let progress_ramp = state
        .frontend
        .loading_session
        .as_ref()
        .and_then(|session| session.native.as_ref())
        .map(|native| native.progress_ramp)
        .ok_or_else(|| anyhow::anyhow!("native loading session lost its progress ramp"))?;
    let prepared_preview = state
        .frontend
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
    let marker_remaps = state
        .frontend
        .loading_session
        .as_ref()
        .and_then(|session| session.native.as_ref())
        .and_then(|native| native.composition.as_ref())
        .zip(state.rules())
        .map(|(composition, rules)| {
            composition
                .markers
                .iter()
                .map(|marker| {
                    let color_key =
                        scheme_entry_for_priority(i32::from(marker.color_priority)) as u8;
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
    let atlas = build_loading_screen_atlas_with_composition(
        &state.renderer.gpu,
        &state.renderer.batch_renderer,
        &assets,
        variant,
        width,
        &progress_ramp,
        composition_input,
    );
    if let Some(native) = state
        .frontend
        .loading_session
        .as_mut()
        .and_then(|session| session.native.as_mut())
    {
        native.atlas = atlas;
    }
    if state
        .frontend
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
    match encode_loading_screen(state, encoder, destination) {
        Ok(result) => result,
        Err(err) => {
            fail_loading(state, LoadingFailurePolicy::ReportNativeFailure, err);
            LoadingRenderResult::Failed
        }
    }
}

fn encode_loading_screen(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<LoadingRenderResult> {
    if !is_native_loading_session(state) {
        return Ok(LoadingRenderResult::GenericFallback);
    }
    if let Err(err) = ensure_native_loading_atlas(state) {
        return Err(err);
    }
    if let Some(session) = state.frontend.loading_session.as_mut()
        && !session.first_frame_presented
        && let Some(native) = session.native.as_mut()
    {
        // Retail composes the LS country surface first, then ProgressClass
        // supplies the first displayed value: selected maps show raw 3, while
        // random-map seed loads halve it to 1.
        native
            .progress
            .advance_progress(native.progress_cadence.effective_percent(3));
    }
    let Some(native) = state
        .frontend
        .loading_session
        .as_ref()
        .and_then(|session| session.native.as_ref())
    else {
        return Ok(LoadingRenderResult::GenericFallback);
    };
    render::encode_native_loading_frame(&state.renderer, native, encoder, destination)?;
    Ok(LoadingRenderResult::NativeRendered)
}

/// Called only after the submitted loading frame and its readbacks. Loading
/// owns acknowledgement, continuation and terminal disposition as one step.
pub(crate) fn after_loading_frame_presented(state: &mut AppState) {
    if !matches!(state.frontend.screen, GameScreen::Loading) {
        return;
    }
    loading_screen_presented(state);
    let policy = if is_native_loading_session(state) {
        LoadingFailurePolicy::ReportNativeFailure
    } else {
        LoadingFailurePolicy::InstallGenericFallback
    };
    match pump_loading_after_present(state) {
        LoadingPump::Pending => state.platform.window.request_redraw(),
        LoadingPump::Finished(result) => {
            log::debug!(target: "vera20k::loading_attempt", "install_begin");
            super::transitions::apply_map_load_result(state, result);
            log::debug!(target: "vera20k::loading_attempt", "install_end screen={:?} assets_available={} accepted={}", state.frontend.screen, state.process_assets.is_available(), state.match_state.startup.accepted().is_some());
        }
        LoadingPump::Failed(err) => fail_loading(state, policy, err),
    }
}

fn fail_loading(state: &mut AppState, policy: LoadingFailurePolicy, err: anyhow::Error) {
    log::warn!("Could not load map: {err:#}");
    let policy = retire_failed_loading_attempt(
        &mut state.frontend.loading_session,
        &mut state.match_state.startup,
        &mut state.process_assets,
        policy,
    );
    reset_loading_presentation(state);
    if policy == LoadingFailurePolicy::ReportNativeFailure {
        state.frontend.screen = GameScreen::MissionResult {
            title: "Loading Failed".to_string(),
            detail: format!("{err:#}"),
        };
    } else {
        super::transitions::apply_map_load_result(
            state,
            super::transitions::fallback_map_load_result(),
        );
    }
}

fn loading_screen_presented(state: &mut AppState) {
    let Some(session) = state.frontend.loading_session.as_mut() else {
        state.frontend.loading_progress.advance_progress(3);
        return;
    };
    session.first_frame_presented = true;
    log::debug!(target: "vera20k::loading_attempt", "frame_presented native_progress={:?}", session.native.as_ref().map(|n| n.progress.current_value()));
}

fn selected_loading_art_variant(state: &AppState) -> Option<LoadingArtVariant> {
    if !matches!(state.frontend.screen, GameScreen::Loading) {
        return None;
    }
    state
        .frontend
        .loading_session
        .as_ref()
        .and_then(|session| session.native.as_ref())
        .map(|native| native.variant)
}

fn loading_art_variant_from_launch_country(country: LaunchCountry) -> LoadingArtVariant {
    match country {
        LaunchCountry::America => LoadingArtVariant::Americans,
        LaunchCountry::Korea => LoadingArtVariant::Alliance,
        LaunchCountry::France => LoadingArtVariant::French,
        LaunchCountry::Germany => LoadingArtVariant::Germans,
        LaunchCountry::GreatBritain => LoadingArtVariant::British,
        LaunchCountry::Libya => LoadingArtVariant::Africans,
        LaunchCountry::Iraq => LoadingArtVariant::Arabs,
        LaunchCountry::Cuba => LoadingArtVariant::Confederation,
        LaunchCountry::Russia => LoadingArtVariant::Russians,
        LaunchCountry::Yuri => LoadingArtVariant::Yuri,
    }
}

/// Normalize an 8-bit RGB triple to 0..1.
fn normalize_rgb(rgb: [u8; 3]) -> [f32; 3] {
    [
        rgb[0] as f32 / 255.0,
        rgb[1] as f32 / 255.0,
        rgb[2] as f32 / 255.0,
    ]
}

fn advance_and_present_native_progress(
    gpu: &GpuContext,
    presenter: &ShellSurfacePresenter,
    depth_view: &wgpu::TextureView,
    batch: &BatchRenderer,
    font: &BitFont,
    native: &mut NativeLoadingScreenState,
    raw_percent: u32,
    render_size: [u32; 2],
) {
    let effective_percent = native.progress_cadence.effective_percent(raw_percent);
    if !native.progress.advance_progress(effective_percent) {
        return;
    }
    let Some(atlas) = native.atlas.as_ref() else {
        return;
    };
    log::debug!(target: "vera20k::loading_attempt", "progress_present raw={raw_percent} effective={effective_percent}");
    if let Err(err) = render::present_native_loading(
        gpu,
        presenter,
        depth_view,
        batch,
        font,
        atlas,
        native.composition.as_ref(),
        &native.progress_row,
        &native.progress,
        native.backing_rgb,
        native.text_rgb,
        render_size,
    ) {
        log::warn!(
            "Native loading repaint at raw milestone {raw_percent} \
             (effective {effective_percent}) failed: {err:#}"
        );
    }
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
    progress: &'a mut LoadingProgressState,
    progress_row: &'a LoadingProgressRowSnapshot,
    atlas: &'a LoadingScreenAtlas,
    composition: Option<&'a LoadingCompositionSnapshot>,
    backing_rgb: [f32; 3],
    text_rgb: [f32; 3],
    render_size: [u32; 2],
    cadence: NativeLoadingProgressCadence,
}

impl LoadingProgressSink for RenderingProgressSink<'_> {
    fn milestone(&mut self, raw_percent: u32) {
        let effective_percent = self.cadence.effective_percent(raw_percent);
        if self.progress.advance_progress(effective_percent) {
            log::debug!(target: "vera20k::loading_attempt", "progress_present raw={raw_percent} effective={effective_percent}");
            if let Err(err) = render::present_native_loading(
                self.gpu,
                self.presenter,
                self.depth_view,
                self.batch,
                self.font,
                self.atlas,
                self.composition,
                self.progress_row,
                self.progress,
                self.backing_rgb,
                self.text_rgb,
                self.render_size,
            ) {
                log::warn!(
                    "Native loading repaint at raw milestone {raw_percent} \
                     (effective {effective_percent}) failed: {err:#}"
                );
            }
        }
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
