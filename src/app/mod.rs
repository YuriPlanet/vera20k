//! Application facade and shared imports for the focused orchestrator modules
//! under `app/`. GPU initialization remains deferred to `resumed()`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::{Window, WindowAttributes, WindowId};

use crate::app::presentation::render;
use crate::app::match_runtime::sim_tick;
use crate::app::loading::transitions;
use crate::assets::asset_manager::AssetManager;
use crate::audio::music::MusicPlayer;
use crate::audio::sfx::SfxPlayer;
use crate::map::basic::BasicSection;
use crate::map::houses::HouseRoster;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::render::batch::BatchRenderer;
use crate::render::bit_font::BitFont;
use crate::render::egui_integration::EguiIntegration;
use crate::render::gpu::GpuContext;
use crate::sidebar::{SidebarChromeLayoutSpec, SidebarTab};
use crate::sim::selection::SelectionState;
use crate::ui::game_screen::GameScreen;
use crate::ui::main_menu::{self};
use crate::ui::shell::controller::ShellKey;
use crate::ui::skirmish_shell::{SavedSeedBrowserState, SavedSeedMode};
use crate::util::config::GameConfig;

pub mod frontend;
pub(crate) mod types;
pub(crate) mod input;
mod frame;
mod handler;
mod initialize;
mod in_game;
pub(crate) mod match_audio;
pub(crate) mod match_runtime;
pub(crate) mod audio_runtime;
pub(crate) mod diagnostics;
pub(crate) mod loading;
pub(crate) mod match_diagnostics;
pub(crate) mod persistence;
pub(crate) mod presentation;
pub(crate) mod process_assets;
pub(crate) mod renderer_state;
pub(crate) mod scenario_catalog;
mod shell_main_menu;
mod shell_movies_credits;
mod shell_random_map;
mod shell_saved_seeds;
mod shell_saved_games;
#[cfg(test)]
mod random_map_lifecycle_tests;
pub(crate) mod shell_route;
mod shell_skirmish;
pub(crate) mod sidebar_projection;
mod state;
mod window_icon;

pub(crate) use shell_random_map::{
    RandomMapGenerationRetention,
};
pub(crate) use state::{AppState, PlatformState, reset_scenario_exit_runtime};

const DEV_SKIRMISH_SHELL_ENV: &str = "RA2_DEV_SKIRMISH_SHELL";

/// Top-level application. Implements winit's ApplicationHandler.
pub struct App {
    state: Option<AppState>,
    shell_capture: Option<crate::app::diagnostics::shell_capture::ShellCaptureSession>,
    tactical_capture: Option<crate::app::diagnostics::tactical_capture::session::TacticalCaptureSession>,
    /// Retained until `resumed()` so the profile's pre-window Video read can
    /// use the command-line screen fields as its current-value defaults.
    startup_options: crate::app::frontend::startup_options::RetailStartupOptions,
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

/// Keep initial and later scenario audio-index loads on the same process-start
/// decision. This deliberately does not inspect the optional output players:
/// an enabled DirectSound/output initialization may independently fail.
pub(crate) const fn should_load_audio_indices(audio_indices_enabled: bool) -> bool {
    audio_indices_enabled
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
    pub fn new(startup_options: crate::app::frontend::startup_options::RetailStartupOptions) -> Self {
        Self {
            state: None,
            shell_capture: None,
            tactical_capture: None,
            startup_options,
        }
    }

    pub fn new_shell_capture(request: crate::app::diagnostics::shell_capture::ShellCaptureRequest) -> Self {
        Self {
            state: None,
            shell_capture: Some(crate::app::diagnostics::shell_capture::ShellCaptureSession::new(request)),
            tactical_capture: None,
            startup_options: crate::app::frontend::startup_options::RetailStartupOptions::default(),
        }
    }

    pub fn new_tactical_capture(request: crate::app::frontend::launch::TacticalCaptureRequest) -> Self {
        Self {
            state: None,
            shell_capture: None,
            tactical_capture: Some(
                crate::app::diagnostics::tactical_capture::session::TacticalCaptureSession::new(request),
            ),
            startup_options: crate::app::frontend::startup_options::RetailStartupOptions::default(),
        }
    }

    pub fn finish_capture(&mut self) -> Result<()> {
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
    fn gsi_01_01_audio_index_decision_survives_scenario_transitions() {
        for audio_enabled in [false, true] {
            let startup = StartupAudioDisposition::for_audio_enabled(audio_enabled);
            let persisted_in_state = startup.load_audio_indices;

            assert_eq!(
                should_load_audio_indices(startup.load_audio_indices),
                audio_enabled
            );
            assert_eq!(should_load_audio_indices(persisted_in_state), audio_enabled);
        }
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
