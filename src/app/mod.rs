//! Application facade and shared imports for the focused orchestrator modules
//! under `app/`. GPU initialization remains deferred to `resumed()`.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use crate::util::time::Instant;

use anyhow::Result;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
#[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
mod web_keyboard;
#[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
use web_keyboard::KeyEventExtModifierSupplement;
use winit::window::{Window, WindowAttributes, WindowId};

use crate::app::match_runtime::sim_tick;
use crate::app::presentation::render;
use crate::assets::asset_manager::AssetManager;
use crate::audio::music::MusicPlayer;
use crate::audio::sfx::SfxPlayer;
use crate::map::basic::BasicSection;
use crate::map::houses::HouseRoster;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::render::batch::BatchRenderer;
use crate::render::bit_font::BitFont;
use crate::render::gpu::GpuContext;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::ui::game_screen::GameScreen;
use crate::ui::shell::controller::ShellKey;
use crate::ui::sidebar::{SidebarChromeLayoutSpec, SidebarTab};
use crate::ui::skirmish_shell::{SavedSeedBrowserState, SavedSeedMode};
use crate::util::config::GameConfig;

pub mod app_bundle;
pub(crate) mod audio_runtime;
pub(crate) mod campaign_defeat;
pub(crate) mod campaign_victory;
pub(crate) mod chrono_screen;
mod cncnet;
mod cooperative_round;
#[cfg(feature = "debug-tools")]
pub(crate) mod debug_tools;
pub(crate) mod diagnostics;
/// Which screen the game opens on: the Display list's own translation between
/// VERA's profile value and the window system's monitors.
pub(crate) mod display_target;
mod frame;
pub mod frontend;
mod game_data_check;
mod handler;
mod in_game;
mod initialize;
pub(crate) mod input;
pub(crate) mod lan_service;
pub(crate) mod loading;
pub(crate) mod match_audio;
pub mod match_bootstrap;
pub(crate) mod match_diagnostics;
pub(crate) mod match_runtime;
pub(crate) mod movie_playback;
pub(crate) mod persistence;
/// The build description's native version metadata (`Phobos.version.h`).
pub(crate) mod phobos_version;
pub(crate) mod presentation;
pub(crate) mod process_assets;
#[cfg(feature = "telemetry-dashboard")]
pub(crate) mod process_stats;
pub(crate) mod radar_movie;
#[cfg(test)]
mod random_map_lifecycle_tests;
pub(crate) mod renderer_state;
pub(crate) mod scenario_catalog;
mod shell_campaign;
pub(crate) mod shell_lan;
mod shell_load_saved_game;
mod shell_main_menu;
mod shell_movies_credits;
pub(crate) mod shell_network_setup;
mod shell_random_map;
pub(crate) mod shell_route;
mod shell_saved_games;
mod shell_saved_seeds;
mod shell_skirmish;
mod shell_wol;
pub(crate) mod sidebar_projection;
#[cfg(test)]
mod sidebar_projection_tests;
mod state;
#[cfg(feature = "telemetry-dashboard")]
pub mod telemetry;
#[cfg(feature = "telemetry-dashboard")]
pub(crate) mod telemetry_capture;
#[cfg(feature = "telemetry-dashboard")]
pub(crate) mod telemetry_server;
pub(crate) mod types;
mod window_icon;

/// Main window title. The single-instance gate finds a running copy's window by
/// this exact title, so `main` hands the same constant to
/// `util::single_instance::acquire`.
pub const WINDOW_TITLE: &str = match (
    crate::rules::ARES_ENABLED,
    crate::rules::PHOBOS_ENABLED,
    crate::rules::NPATCH_ENABLED,
) {
    (true, true, true) => "VERA20K [Ares+Phobos+NPatch]",
    (true, true, false) => "VERA20K [Ares+Phobos]",
    (true, false, true) => "VERA20K [Ares+NPatch]",
    (true, false, false) => "VERA20K [Ares]",
    (false, true, true) => "VERA20K [Phobos+NPatch]",
    (false, true, false) => "VERA20K [Phobos]",
    (false, false, true) => "VERA20K [NPatch]",
    (false, false, false) => "VERA20K",
};

/// Identity stamp shared by the status screen and the telemetry capture. Not a
/// retail build string, and not the tactical view's version description —
/// that one is Phobos's own text ([`phobos_version`]).
pub(crate) const BUILD_STAMP: &str = concat!("VERA20K ", env!("CARGO_PKG_VERSION"));

pub(crate) use shell_random_map::RandomMapGenerationRetention;
pub(crate) use state::{AppState, PlatformState, reset_scenario_exit_runtime};

/// Top-level application. Implements winit's ApplicationHandler.
pub struct App {
    state: Option<AppState>,
    pending_initialization: Option<initialize::InitializationJob>,
    shell_capture: Option<crate::app::diagnostics::shell_capture::ShellCaptureSession>,
    tactical_capture:
        Option<crate::app::diagnostics::tactical_capture::session::TacticalCaptureSession>,
    /// Render admission for hidden captures; service events may wake earlier.
    capture_render_not_before: Option<Instant>,
    /// Retained until `resumed()` so the profile's pre-window Video read can
    /// use the command-line screen fields as its current-value defaults.
    startup_options: crate::app::frontend::startup_options::RetailStartupOptions,
    spawn_request: Option<crate::app::frontend::cncnet_spawn::SpawnRequest>,
    replay_request: Option<crate::app::frontend::launch::ReplayLaunchRequest>,
    startup_failure: Option<String>,
    /// Make sure config.toml names a game folder before the window opens
    /// ([`app_bundle`]).
    ask_for_game_folder: bool,
    /// Set while `window_event` re-dispatches the mouse events a touch meant
    /// (`input::touch`). A synthetic `CursorMoved` must not pass for the OS
    /// pointer, which is what clears the touch-parked cursor.
    dispatching_touch: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StartupAudioDisposition {
    initialize_music_output: bool,
    initialize_sfx_output: bool,
    load_audio_indices: bool,
}

impl StartupAudioDisposition {
    /// gamemd-derived: `FUN_0052F620 @ 0x0052F620` clears the audio global for
    /// `-NOAUDIO`; active `Init_Game @ 0x0052BA60` passes that value to
    /// `AudioSystem::Init @ 0x00406B10`, whose false branch skips output and
    /// audio MIX/index construction while the later bookkeeping remains live.
    const fn for_audio_enabled(audio_enabled: bool) -> Self {
        Self {
            initialize_music_output: audio_enabled,
            initialize_sfx_output: audio_enabled,
            load_audio_indices: audio_enabled,
        }
    }
}

impl Default for StartupAudioDisposition {
    fn default() -> Self {
        Self::for_audio_enabled(true)
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new(crate::app::frontend::startup_options::RetailStartupOptions::default())
    }
}

impl App {
    pub fn new(
        startup_options: crate::app::frontend::startup_options::RetailStartupOptions,
    ) -> Self {
        Self {
            state: None,
            pending_initialization: None,
            shell_capture: None,
            tactical_capture: None,
            capture_render_not_before: None,
            startup_options,
            spawn_request: None,
            replay_request: None,
            startup_failure: None,
            ask_for_game_folder: false,
            dispatching_touch: false,
        }
    }

    pub fn new_shell_capture(
        request: crate::app::diagnostics::shell_capture::ShellCaptureRequest,
    ) -> Self {
        Self {
            state: None,
            pending_initialization: None,
            shell_capture: Some(
                crate::app::diagnostics::shell_capture::ShellCaptureSession::new(request),
            ),
            tactical_capture: None,
            capture_render_not_before: None,
            startup_options: crate::app::frontend::startup_options::RetailStartupOptions::default(),
            spawn_request: None,
            replay_request: None,
            startup_failure: None,
            ask_for_game_folder: false,
            dispatching_touch: false,
        }
    }

    pub fn new_tactical_capture(
        request: crate::app::frontend::launch::TacticalCaptureRequest,
    ) -> Self {
        Self {
            state: None,
            pending_initialization: None,
            shell_capture: None,
            tactical_capture: Some(
                crate::app::diagnostics::tactical_capture::session::TacticalCaptureSession::new(
                    request,
                ),
            ),
            capture_render_not_before: None,
            startup_options: crate::app::frontend::startup_options::RetailStartupOptions::default(),
            spawn_request: None,
            replay_request: None,
            startup_failure: None,
            ask_for_game_folder: false,
            dispatching_touch: false,
        }
    }

    /// Before the window opens, ask for the Yuri's Revenge folder until
    /// config.toml names one: a launch from the macOS app bundle or the Linux
    /// AppImage.
    pub fn ask_for_game_folder_first(&mut self) {
        self.ask_for_game_folder = true;
    }

    pub fn new_replay(request: crate::app::frontend::launch::ReplayLaunchRequest) -> Self {
        let mut app = Self::default();
        app.replay_request = Some(request);
        app
    }

    pub fn finish_capture(&mut self) -> Result<()> {
        if let Some(error) = self.startup_failure.as_ref().or_else(|| {
            self.state
                .as_ref()
                .and_then(|state| state.frontend.external_launch_error.as_ref())
        }) {
            anyhow::bail!("{error}");
        }
        match (self.shell_capture.as_mut(), self.tactical_capture.as_mut()) {
            (Some(session), None) => session.take_outcome(),
            (None, Some(session)) => session.take_outcome(),
            (None, None) => Ok(()),
            (Some(_), Some(_)) => anyhow::bail!("multiple capture modes were active"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gsi_01_01_noaudio_suppresses_music_and_sfx_output() {
        let disposition = StartupAudioDisposition::for_audio_enabled(false);

        assert!(!disposition.initialize_music_output);
        assert!(!disposition.initialize_sfx_output);
        assert!(!disposition.load_audio_indices);
    }

    #[test]
    fn gsi_01_01_default_startup_enables_music_and_sfx_output() {
        let disposition = StartupAudioDisposition::default();

        assert!(disposition.initialize_music_output);
        assert!(disposition.initialize_sfx_output);
        assert!(disposition.load_audio_indices);
    }

    #[test]
    fn options_profile_startup_fields_survive_until_resumed() {
        let startup = crate::app::frontend::startup_options::RetailStartupOptions {
            screen_width: 640,
            screen_height: 480,
            audio_enabled: false,
            ..Default::default()
        };
        let app = App::new(startup);

        assert_eq!(app.startup_options, startup);
    }
}
