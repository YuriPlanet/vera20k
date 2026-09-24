//! Frontend owner (F12 `FrontendState`): shell/menu/score/dialog presentation
//! and flow state outside a running match.
//!
//! Everything here is app-side and never read by the deterministic simulation.
//! Match-lifetime state lives in the match owners; process-lifetime GPU and
//! audio objects live in `RendererState` / `AppAudioRuntime`.

use std::time::Instant;

use super::startup_splash;
use crate::app::loading::init::MapMenuEntry;
use crate::app::shell_random_map::{RandomMapGenerationJob, RandomMapGenerationRetention};
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::ui::game_screen::GameScreen;
use crate::ui::main_menu::SkirmishSettings;

pub(crate) struct FrontendState {
    /// Opt-in research shell path. Defaults off so the egui Skirmish setup is visible.
    pub(crate) dev_skirmish_shell_enabled: bool,
    pub(crate) skirmish_shell_state: crate::ui::skirmish_shell::SkirmishShellState,
    /// Process-lifetime offline shell snapshot, Scenario cursor, and
    /// Cooperative progress authority.
    pub(crate) offline_skirmish_runtime: crate::app::frontend::skirmish_session::OfflineSkirmishRuntime,
    /// Last owner-draw Skirmish button state observed by the native render path.
    /// Used for the retail GenericClick paint-transition sound.
    pub(crate) skirmish_shell_last_painted_pressed_button:
        Option<crate::ui::skirmish_shell::OwnerDrawButton>,
    pub(crate) skirmish_shell_chrome:
        Option<crate::render::skirmish_shell_chrome::SkirmishShellChromeAtlas>,
    pub(crate) main_menu_shell_state: crate::ui::main_menu_shell::MainMenuShellState,
    pub(crate) single_player_shell_state: crate::ui::single_player_shell::SinglePlayerShellState,
    /// Shared descriptor-driven input authority for the front-end shell dialogs
    /// (0xE2 main menu, 0x100 single player). Owns hit-test + press-must-match;
    /// its press/hover state is mirrored back into the per-shell structs above for
    /// the render path (substrate Slice 2).
    pub(crate) shell_controller: crate::ui::shell::controller::DialogController,
    pub(crate) main_menu_shell_chrome:
        Option<crate::render::main_menu_shell_chrome::MainMenuShellChromeAtlas>,
    pub(crate) main_menu_movie: Option<crate::render::bink_movie::BinkMovieSurface>,
    pub(crate) main_menu_movie_identity:
        Option<crate::app::frontend::main_menu_shell_render::Ra2tsMovieSessionIdentity>,
    pub(crate) main_menu_movie_last_step: Instant,
    pub(crate) main_menu_shell_failed: bool,
    /// Numeric internal-version string used by the bottom-right main-menu line.
    /// Resolution follows the retail 16-byte/CR-only cached contract.
    pub(crate) version_txt: String,
    /// Active shell first-paint controls-reveal slide (presentation only). gamemd
    /// plays this on the first paint of every shell dialog (menu / single-player /
    /// skirmish); the wave swaps each owner-draw button's SDBTNANM frame index.
    pub(crate) shell_first_paint_slide: Option<crate::app::frontend::shell_transition::ShellFrameWave>,
    /// Which shell dialog the first-paint slide last fired for. Drives per-frame
    /// edge detection so the slide (re)starts on entry into each shell and is
    /// cancelled on leaving all of them.
    pub(crate) shell_slide_active_shell: Option<crate::app::frontend::shell_transition::ShellSlideKind>,
    /// Monotonic identity for each newly armed exact Main Menu `0xE2` instance.
    pub(crate) shell_slide_generation: u64,
    /// Active graceful quit cascade (music fade → trailing-voice wait → hard stop
    /// → exit). Some only between Exit-confirm OK and window close; freezes shell
    /// input while it runs.
    pub(crate) quit_cascade: Option<crate::app::frontend::quit_cascade::QuitCascade>,
    /// Retail process-start splash, held until its post-present deadline.
    pub(crate) startup_splash: Option<startup_splash::StartupSplashPresentation>,
    /// Exit-Game confirm message box, open while the player is being asked to
    /// confirm quitting. The app only exits on confirm, never on the first
    /// Exit click.
    pub(crate) exit_confirm_modal: Option<crate::ui::main_menu_dialogs::ExitConfirmModalState>,
    /// Retained active-YR launcher Options `0xD5` parent snapshot.
    pub(crate) options_dialog: Option<crate::ui::main_menu_dialogs::OptionsDialogState>,
    /// Shared A3 child, reached from launcher Options or paused Game Controls.
    pub(crate) keyboard_dialog: Option<crate::ui::shell::keyboard::KeyboardState>,
    pub(crate) launcher_options_presentation: super::skirmish_shell_render::LauncherOptionsPresentation,
    /// Movie list `0x129` instance while `ShellRoute::MovieList` is shown.
    pub(crate) movie_list: Option<crate::ui::movies_credits_shell::MovieListState>,
    /// `DAT_00825C80`: the list row last played, retained for the process
    /// (initially -1) and reapplied when `0x129` is recreated.
    pub(crate) movie_list_selection: i32,
    /// Full-screen Play_Movie session (Sneak Peeks or a movie-list entry).
    pub(crate) fullscreen_movie: Option<crate::app::frontend::fullscreen_movie::FullscreenMovie>,
    /// Show_Credits session.
    pub(crate) credits_roll: Option<crate::app::frontend::credits_roll::CreditsRollSession>,
    /// Campaign selector dialog (Single Player -> New Campaign; launch mapping
    /// not decoded).
    pub(crate) campaign_select: Option<crate::ui::main_menu_dialogs::CampaignSelectState>,
    /// End-of-match score presentation, decorated from the sim-owned terminal
    /// snapshot and held until the player leaves the screen. `None` for result
    /// screens with no native score analogue (a load failure, a trigger-driven
    /// campaign end), which keep the non-art fallback.
    pub(crate) score_screen: Option<crate::ui::score_shell::ScoreScreenModel>,
    pub(crate) score_shell_state: crate::ui::score_shell::ScoreShellState,
    /// Number of matches finished this session — the score screen's `Game: n`.
    /// gamemd increments the same counter as it tears the scenario down.
    pub(crate) finished_game_count: u32,
    /// Which shell surface owns the MainMenu screen (F11): structural
    /// exclusivity replaces the old boolean triple.
    pub(crate) shell_route: crate::app::shell_route::ShellRoute,
    /// Which screen is currently active (MainMenu, Loading, InGame).
    pub(crate) screen: GameScreen,
    /// Available maps from the RA2 directory for menu selection.
    pub(crate) available_maps: Vec<MapMenuEntry>,
    /// Scenario records + their projected shell map entries (F11): one owner,
    /// projection re-derived on every mutation so indices cannot drift.
    pub(crate) scenario_catalog: crate::app::scenario_catalog::ScenarioCatalog,
    /// MPModes rows used by the native Choose Map modal.
    pub(crate) skirmish_modes: Vec<crate::skirmish_modes::SkirmishGameMode>,
    /// Player-configured skirmish settings (map, country, credits, etc.).
    pub(crate) skirmish_settings: SkirmishSettings,
    pub(crate) loading_session: Option<crate::app::loading::pump::LoadingSession>,
    /// Process-owned front-end Main stream. RMG Randomize/derived-option work
    /// and the first preview selector reach share this cursor; the setup-entry
    /// seed comes from shell Scenario instead. Accepted matches reseed their
    /// own Main stream rather than inheriting either shell cursor.
    pub(crate) frontend_main_rng: crate::sim::rng::SimRng,
    /// UI-thread CRT state shared by storage dialogs, preserved across routes.
    /// Native tactical sparkle/network consumption is still an integration gap;
    /// do not infer exact post-game filenames from this initial state.
    pub(crate) legacy_crt_rng: crate::util::legacy_crt_rng::LegacyCrtRng,
    /// Process-lifetime monotonic identity source; zero is permanently reserved.
    pub(crate) next_match_correlation: u64,
    /// Generation running on a worker, if any. Generating a map takes long
    /// enough to freeze the window if done inline, which also means the
    /// dialog's "Working / Please Wait" never gets a frame to appear in.
    pub(crate) random_map_generation: Option<RandomMapGenerationJob>,
    /// Exact setup-generated map retained through OK until loading owns it.
    pub(crate) random_map_retention: RandomMapGenerationRetention,
    pub(crate) skirmish_preview_texture:
        Option<crate::app::frontend::skirmish_shell_render::SkirmishPreviewTexture>,
    /// Minimap renderer — created at map load time.
    pub(crate) loading_screen_atlas:
        Option<crate::render::loading_screen_chrome::LoadingScreenAtlas>,
    pub(crate) loading_progress: crate::app::loading::pump::LoadingProgressState,
    /// Game data from rules.ini — needed by combat system for weapon/warhead lookups.
    /// Startup-shell rules loaded at boot for menu presentation; match paths
    /// read the runtime-bound copy via `rules()`.
    pub(crate) frontend_rules: Option<crate::rules::ruleset::RuleSet>,
    /// Shell-retained overlay registry for the random-map preview: keeps the
    /// last-loaded match registry across scenario exit, matching the pre-F07
    /// persistence of the old app field. Match paths read the runtime-bound
    /// copy via `overlay_registry()`.
    pub(crate) shell_preview_overlay_registry: Option<OverlayTypeRegistry>,
}
