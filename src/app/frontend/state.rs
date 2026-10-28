//! Frontend owner (F12 `FrontendState`): shell/menu/score/dialog presentation
//! and flow state outside a running match.
//!
//! Everything here is app-side and never read by the deterministic simulation.
//! Match-lifetime state lives in the match owners; process-lifetime GPU and
//! audio objects live in `RendererState` / `AppAudioRuntime`.

use crate::util::time::Instant;

use super::startup_splash;
use crate::app::shell_random_map::{RandomMapGenerationJob, RandomMapGenerationRetention};
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::ui::game_screen::GameScreen;

pub(crate) struct FrontendState {
    /// Immutable process flag from Parse_Command_Line52F94B (0xA8EDDC).
    pub(crate) campaign_speed_control: bool,
    /// External offline launch: returning from the session closes the process for its client.
    pub(crate) external_launch: bool,
    pub(crate) external_launch_error: Option<String>,
    /// The external launch's local result, first decided wins
    /// (docs/spawn.md); see `App::external_match_result`.
    pub(crate) external_match_result: Option<crate::app::cncnet::ExternalMatchResult>,
    /// Frames, duration and opponent departure for the launch report.
    pub(crate) external_progress: crate::app::cncnet::ExternalLaunchProgress,
    /// Browser only: an ended external launch waits on the menu backdrop,
    /// without shell input, for the host's next `start_spawn` (docs/spawn.md).
    pub(crate) external_idle: bool,
    /// The Skirmish setup dialog's state.
    pub(crate) skirmish_shell_state: crate::ui::skirmish_shell::SkirmishShellState,
    /// Choose Map's last clicked game-type row `[0x008316FC]`: starts at -1,
    /// written only by a game-type click and never reset, so it outlives
    /// every chooser visit.
    pub(crate) choose_map_last_mode_row: Option<usize>,
    /// Process-lifetime offline shell snapshot, Scenario cursor, and
    /// Cooperative progress authority.
    pub(crate) multiplayer_shell_runtime:
        crate::app::frontend::skirmish_session::MultiplayerShellRuntime,
    pub(crate) skirmish_shell_chrome:
        Option<crate::render::skirmish_shell_chrome::SkirmishShellChromeAtlas>,
    /// The `DialogBackground.Image/.Palette` pair the shell chrome atlas was
    /// built with. A game load that resolves a different pair drops the atlas
    /// so it rebuilds with the side's three dialog slots.
    pub(crate) shell_dialog_background: Option<(String, String)>,
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
    /// Terminal menu failure; retained for the error screen and input gate.
    pub(crate) main_menu_shell_error: Option<String>,
    /// Set by each status-screen draw and taken by the frame's cursor owner:
    /// the card has no retail art, so the window's own cursor shows over it.
    pub(crate) status_screen_drawn: bool,
    /// Numeric internal-version string used by the bottom-right main-menu line.
    /// Resolution follows the retail 16-byte/CR-only cached contract.
    pub(crate) version_txt: String,
    /// Active shell first-paint controls-reveal slide (presentation only). gamemd
    /// plays this on the first paint of every shell dialog (menu / single-player /
    /// skirmish); the wave swaps each owner-draw button's SDBTNANM frame index.
    pub(crate) shell_first_paint_slide:
        Option<crate::app::frontend::shell_transition::ShellFrameWave>,
    /// Which shell dialog the first-paint slide last fired for. Drives per-frame
    /// edge detection so the slide (re)starts on entry into each shell and is
    /// cancelled on leaving all of them.
    pub(crate) shell_slide_active_shell:
        Option<crate::app::frontend::shell_transition::ShellSlideKind>,
    /// Monotonic identity for each newly armed exact Main Menu `0xE2` instance.
    pub(crate) shell_slide_generation: u64,
    /// Static `0x71C` of the showing main-menu family dialog (`0xE2`, `0x100`,
    /// `0x101`, `0x129`). Reset for every new dialog instance by the shell
    /// lifecycle reducer; drawn by the family renderers and committed after
    /// present.
    pub(crate) shell_monitor: crate::ui::shell::warning_monitor::WarningMonitor,
    /// Heading static `0x694` of the showing menu page (`0x100`, `0x101`,
    /// `0x129`): hidden until the page's first-paint slide completes, then a
    /// kind-1 reveal. Main menu `0xE2` keeps its own presented-entry reveal.
    pub(crate) shell_page_title: crate::ui::shell::static_reveal::PresentedKind1Static,
    /// Status line `0x695` of the showing main-menu family dialog (`0xE2`,
    /// `0x100`, `0x101`, `0x129`): hover help, hidden until the dialog's SHOW
    /// completion, then a kind-1 reveal that restarts on every text change.
    pub(crate) shell_status_line: crate::ui::shell::static_reveal::PresentedKind1Static,
    /// The showing family dialog's teardown slide-out while it runs; its
    /// continuation commits the route change when the slide ends.
    pub(crate) shell_exit: Option<crate::app::frontend::shell_transition::ShellExit>,
    /// Active graceful quit cascade (music fade → trailing-voice wait → hard stop
    /// → exit). Some only between Exit-confirm OK and window close; freezes shell
    /// input while it runs.
    pub(crate) quit_cascade: Option<crate::app::frontend::quit_cascade::QuitCascade>,
    /// Retail process-start splash, held until its post-present deadline.
    pub(crate) startup_splash: Option<startup_splash::StartupSplashPresentation>,
    /// The two-button shell message box `0x120` over the empty backdrop:
    /// main-menu Exit Game (the app only exits on its OK, never on the first
    /// Exit click) or campaign defeat's retry prompt.
    pub(crate) shell_message_box: Option<ShellMessageBox>,
    /// Retained active-YR launcher Options `0xD5` parent snapshot.
    pub(crate) options_dialog: Option<crate::ui::main_menu_dialogs::OptionsDialogState>,
    /// Shared A3 child, reached from launcher Options or paused Game Controls.
    pub(crate) keyboard_dialog: Option<crate::ui::shell::keyboard::KeyboardState>,
    /// Movie list `0x129` instance while `ShellRoute::MovieList` is shown.
    pub(crate) movie_list: Option<crate::ui::movies_credits_shell::MovieListState>,
    /// `DAT_00825C80`: the list row last played, retained for the process
    /// (initially -1) and reapplied when `0x129` is recreated.
    pub(crate) movie_list_selection: i32,
    /// Full-screen Play_Movie session (Sneak Peeks or a movie-list entry).
    pub(crate) fullscreen_movie: Option<crate::app::frontend::fullscreen_movie::FullscreenMovie>,
    /// Show_Credits session.
    pub(crate) credits_roll: Option<crate::app::frontend::credits_roll::CreditsRollSession>,
    /// Campaign selection `0x94` instance while its route shows.
    pub(crate) campaign: Option<crate::ui::campaign_shell::CampaignShellState>,
    /// Its emblem and background art (`0x0072D9A0`), loaded with the dialog
    /// and freed when it closes (`0x0072DAA0`).
    pub(crate) campaign_art: Option<crate::render::main_menu_shell_chrome::CampaignShellArt>,
    /// One Westwood Online visit (`WOL_Main` `0x0077B2A0`): welcome, login,
    /// waits, boxes and the lobby entry, from Internet until WOL is left.
    pub(crate) wol: Option<crate::app::shell_wol::WolVisit>,
    /// Retains only the departing online visit's bounded HTTP cleanup owner.
    pub(crate) online_cleanup: crate::app::shell_wol::OnlineCleanup,
    /// The login chain's process globals (nickname, password, Remember,
    /// changed byte); they outlive every WOL visit.
    pub(crate) wol_account: crate::ui::wol_login::WolAccountGlobals,
    /// The offline WOL test service's credential store, standing in for the
    /// WOLAPI one for the process.
    pub(crate) wol_test_credentials: Vec<crate::ui::wol_login::WolCredential>,
    /// LAN lobby interaction/discovery; admitted participants belong to MatchSession.
    pub(crate) lan: Option<crate::app::shell_lan::LanShellRuntime>,
    pub(crate) network_setup_protocol:
        Option<crate::app::shell_network_setup::NetworkSetupProtocol>,
    /// BC/BD control interaction; values are projected from MatchSession.
    pub(crate) multiplayer_setup: Option<crate::ui::multiplayer_setup::MultiplayerSetupState>,
    /// Options' Network child D7, retained until its teardown completes.
    pub(crate) network_options: Option<crate::ui::network_options::NetworkOptionsState>,
    /// `0x10E`'s background and icon art (`0x0072C7E0`).
    pub(crate) wol_welcome_art: Option<crate::render::main_menu_shell_chrome::WolPageArt>,
    /// `0x10F`'s background art (`0x0072C600`), freed with the dialog
    /// (`0x0072C650`).
    pub(crate) wol_login_art: Option<crate::render::main_menu_shell_chrome::WolPageArt>,
    /// `0x113`'s background and list art (`0x0072CD60`).
    pub(crate) wol_lobby_art: Option<crate::render::main_menu_shell_chrome::WolPageArt>,
    /// WOL Options `0x2BC`'s art (`0x0072BAC0`) and its Sound Options
    /// `0xD6`'s (`0x0072B8E0`).
    pub(crate) wol_options_art: Option<crate::render::main_menu_shell_chrome::WolPageArt>,
    /// `0x0072BCA0`: AutoLoginQuery.shp through its PAL while `0x112` runs.
    pub(crate) wol_auto_login_art: Option<crate::render::main_menu_shell_chrome::WolPageArt>,
    /// The Buddy List `0x116`'s art (`0x0072C060`).
    pub(crate) wol_buddy_art: Option<crate::render::main_menu_shell_chrome::WolPageArt>,
    /// `0x117`'s quickmatch.shp/.PAL (`0x0072BE80`).
    pub(crate) wol_quick_match_art: Option<crate::render::main_menu_shell_chrome::WolPageArt>,
    /// The buddy mirror and its queues (process globals `0xB73AA8`..).
    pub(crate) wol_buddies: crate::app::shell_wol::buddies::WolBuddies,
    /// The offline WOL test service's buddy list, standing in for the
    /// server-side one for the process.
    pub(crate) wol_test_buddies: Vec<String>,
    /// The offline WOL test service's stand-in guest of a created game.
    pub(crate) wol_test_guest: crate::app::shell_wol::WolTestGuest,
    /// My Information `0x114`'s art (`0x0072C9C0`).
    pub(crate) wol_my_info_art: Option<crate::render::main_menu_shell_chrome::WolPageArt>,
    pub(crate) wol_sound_options_art: Option<crate::render::main_menu_shell_chrome::WolPageArt>,
    /// The custom lobby's process globals (sorts, lobby number, filters).
    pub(crate) wol_lobby_globals: crate::app::shell_wol::lobby::WolLobbyGlobals,
    /// The live `[WOnline]` globals (`[0xB76234]`..`[0xB73AD0]`): each WOL
    /// entry copies the profile's (`0x0077DED0`); the chat filter words and
    /// WOL Options' OK change them.
    pub(crate) wol_preferences: crate::ui::wol_options::WolPreferences,
    /// Single Player's Load Saved Game `0xB7` while its route shows.
    pub(crate) load_saved_game:
        Option<crate::ui::skirmish_shell::SavedSeedBrowserState<std::path::PathBuf>>,
    /// The score dialog `0x108` after a finished match, decorated from the
    /// sim-owned terminal snapshot and held until Continue's teardown. `None`
    /// for result screens with no native score analogue (a load failure, a
    /// trigger-driven campaign end), which keep the non-art fallback.
    pub(crate) score_page: Option<crate::ui::score_shell::ScorePage>,
    /// Campaign victory dialog `0x103`, separate from Skirmish score dialog `0x108`.
    pub(crate) campaign_score_page: Option<crate::ui::campaign_score_shell::CampaignScorePage>,
    /// The LAN co-op result dialog `0xBC6` between missions. Its `SCRBK`
    /// background is held in `campaign_score_art`.
    pub(crate) coop_score_page: Option<crate::ui::coop_score_shell::CoopScorePage>,
    /// `0x108`'s side art, loaded for the open page (`0x0072D730`) and
    /// released with it (`0x0072D780`).
    pub(crate) score_art: Option<crate::render::main_menu_shell_chrome::ScoreArt>,
    /// Campaign `0x103` side background, released when its page closes.
    pub(crate) campaign_score_art: Option<crate::render::main_menu_shell_chrome::CampaignScoreArt>,
    /// Number of matches finished this session — the score screen's `Game: n`.
    /// gamemd increments the same counter as it tears the scenario down.
    pub(crate) finished_game_count: u32,
    /// Which shell surface owns the MainMenu screen (F11): structural
    /// exclusivity replaces the old boolean triple.
    pub(crate) shell_route: crate::app::shell_route::ShellRoute,
    /// Which screen is currently active (MainMenu, Loading, InGame).
    pub(crate) screen: GameScreen,
    /// Scenario records + their projected shell map entries (F11): one owner,
    /// projection re-derived on every mutation so indices cannot drift.
    pub(crate) scenario_catalog: crate::app::scenario_catalog::ScenarioCatalog,
    /// MPModes rows used by the native Choose Map modal.
    pub(crate) skirmish_modes: Vec<crate::skirmish_modes::SkirmishGameMode>,
    pub(crate) loading_session: Option<crate::app::loading::pump::LoadingSession>,
    /// Retained process Main cell. A successful fresh install moves its
    /// loading-advanced cursor here and binds the simulation to the same cell.
    /// Audio, Theme and RMG continue that cursor behind the shell; the setup
    /// seed before the first scenario comes from shell Scenario.
    pub(crate) frontend_main_rng: crate::sim::rng::MainRng,
    /// The process start `g_RngSeed` (`0xA8ED94`) before any match reseeds
    /// it; WOL hosts publish the resident seed (C2 draws none).
    pub(crate) startup_rng_seed: u32,
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

/// One open shell message box `0x120` and the caller its result returns to.
#[derive(Debug)]
pub(crate) struct ShellMessageBox {
    pub(crate) labels: crate::ui::main_menu_dialogs::ShellMessageBoxLabels,
    pub(crate) purpose: ShellMessageBoxPurpose,
}

#[derive(Debug)]
pub(crate) enum ShellMessageBoxPurpose {
    /// Main_Game state 6: OK quits, Cancel rebuilds `0xE2`.
    ExitGame,
    /// GameExit::Defeat `0x006860E5`: Yes restarts this start, if VERA can
    /// rebuild one; No returns to the shell.
    CampaignReplay(Option<Box<crate::app::match_bootstrap::PreparedCampaignStartup>>),
    /// MapSelect's failed Init message returns to the victory continuation.
    CampaignVictory(Box<crate::app::campaign_victory::CampaignVictory>),
}
