//! Application facade and shared imports for the focused orchestrator modules
//! under `app/`. GPU initialization remains deferred to `resumed()`.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::{Fullscreen, Window, WindowAttributes, WindowId};

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

pub(crate) mod audio_runtime;
pub(crate) mod campaign_defeat;
pub(crate) mod campaign_victory;
pub(crate) mod chrono_screen;
mod cncnet;
#[cfg(feature = "debug-tools")]
pub(crate) mod debug_tools;
pub(crate) mod diagnostics;
mod frame;
pub mod frontend;
mod handler;
mod in_game;
mod initialize;
pub(crate) mod input;
pub(crate) mod loading;
pub(crate) mod match_audio;
pub mod match_bootstrap;
pub(crate) mod match_diagnostics;
pub(crate) mod match_runtime;
pub(crate) mod movie_playback;
pub(crate) mod persistence;
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
pub const WINDOW_TITLE: &str = if crate::rules::ARES_ENABLED {
    "VERA20K [Ares]"
} else {
    "VERA20K"
};

pub(crate) use shell_random_map::RandomMapGenerationRetention;
pub(crate) use state::{AppState, PlatformState, reset_scenario_exit_runtime};

/// Top-level application. Implements winit's ApplicationHandler.
pub struct App {
    state: Option<AppState>,
    shell_capture: Option<crate::app::diagnostics::shell_capture::ShellCaptureSession>,
    tactical_capture:
        Option<crate::app::diagnostics::tactical_capture::session::TacticalCaptureSession>,
    /// Retained until `resumed()` so the profile's pre-window Video read can
    /// use the command-line screen fields as its current-value defaults.
    startup_options: crate::app::frontend::startup_options::RetailStartupOptions,
    spawn_request: Option<crate::app::frontend::cncnet_spawn::SpawnRequest>,
    startup_failure: Option<String>,
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
            shell_capture: None,
            tactical_capture: None,
            startup_options,
            spawn_request: None,
            startup_failure: None,
        }
    }

    pub fn new_shell_capture(
        request: crate::app::diagnostics::shell_capture::ShellCaptureRequest,
    ) -> Self {
        Self {
            state: None,
            shell_capture: Some(
                crate::app::diagnostics::shell_capture::ShellCaptureSession::new(request),
            ),
            tactical_capture: None,
            startup_options: crate::app::frontend::startup_options::RetailStartupOptions::default(),
            spawn_request: None,
            startup_failure: None,
        }
    }

    pub fn new_tactical_capture(
        request: crate::app::frontend::launch::TacticalCaptureRequest,
    ) -> Self {
        Self {
            state: None,
            shell_capture: None,
            tactical_capture: Some(
                crate::app::diagnostics::tactical_capture::session::TacticalCaptureSession::new(
                    request,
                ),
            ),
            startup_options: crate::app::frontend::startup_options::RetailStartupOptions::default(),
            spawn_request: None,
            startup_failure: None,
        }
    }

    pub fn finish_capture(&mut self) -> Result<()> {
        if let Some(error) = self.startup_failure.as_ref().or_else(|| {
            self.state.as_ref().and_then(|state| state.frontend.external_launch_error.as_ref())
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
