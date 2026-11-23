use super::*;
use crate::app::frontend::menu_page_render::FamilyPage;
use crate::app::frontend::shell_layout;
use crate::app::frontend::shell_transition::ShellSlideKind;
use crate::app::frontend::state::ShellMessageBoxPurpose as Purpose;
use crate::app::match_runtime::sim_host::SimWrite;
use crate::app::shell_route::ShellRoute;
use crate::ui::main_menu_dialogs::options::LauncherParentResult;
use crate::ui::shell::page::PageCue;
use crate::util::time::{Duration, Instant};

impl AppState {
    /// Puts `tag`'s language in use as a pick in the Language list does:
    /// from the next frame every label reads in it, as the string table takes
    /// its texts and the map titles read from the table follow; the fonts
    /// take its script's letter forms and reading direction by themselves
    /// (`TextEngine::follow_interface_language`), as after a reload of the
    /// language files. False, changing nothing, when no language file
    /// provides `tag`.
    pub(crate) fn use_language(&mut self, tag: &str) -> bool {
        if !crate::i18n::select(tag) {
            return false;
        }
        self.refresh_string_key_labels();
        true
    }

    /// Re-resolve the loaded string table and the map titles after a new
    /// language, the String Keys box or its hotkey changed what labels answer.
    pub(crate) fn refresh_string_key_labels(&mut self) {
        if let Some(csf) = self.process_assets.csf.as_mut() {
            crate::i18n::apply(csf);
            crate::map::scenario_sources::relabel_records(
                &mut self.frontend.scenario_catalog.records_mut(),
                csf,
            );
        }
        // The login form keeps its status line as resolved text; the rest of
        // that page resolves every frame.
        if let Some(visit) = self.frontend.wol.as_mut()
            && let crate::app::shell_wol::WolPage::Login(login) = &mut visit.page
        {
            for form in login.form.iter_mut().chain(login.account.iter_mut()) {
                form.refresh_text();
            }
        }
    }

    /// The String Keys hotkey: with the Developer box ticked, keys and texts
    /// swap in place; with it off, the key does nothing.
    pub(crate) fn toggle_string_keys_view(&mut self) {
        if !crate::i18n::string_keys_enabled() {
            return;
        }
        let keys = crate::i18n::toggle_string_keys();
        self.refresh_string_key_labels();
        log::info!("String keys: {}", if keys { "keys" } else { "values" });
        self.platform.window.request_redraw();
    }
}

/// The options profile's `[Video]` bytes behind HIRES and TOGGLE, which the
/// native writer never saves. VERA's Options page has no resolution list to
/// read AllowHiResModes (`0x0056017A..0x005601FA`), and AllowModeToggle is
/// read only at startup, so neither code changes anything else this session.
fn typed_code_video_flags(
    profile: &mut crate::app::persistence::options_profile::RetailOptionsProfile,
) -> crate::ui::main_menu_shell::VideoCodeFlags<'_> {
    crate::ui::main_menu_shell::VideoCodeFlags {
        allow_hi_res_modes: &mut profile.allow_hi_res_modes,
        allow_mode_toggle: &mut profile.allow_mode_toggle,
    }
}

/// `Main__PrepareSession` before a LAN or WOL session.
pub(crate) fn clear_typed_codes_for_network_session(state: &mut AppState) {
    state
        .frontend
        .main_menu_shell_state
        .typed_codes
        .clear_for_network_session(typed_code_video_flags(
            &mut state.persistence.options_profile,
        ));
}

/// The startup error card's title and text, in the language in use: the card
/// draws VERA's bundled fonts, so any script and any path shows as it is.
/// `menu_assets` says the game archives loaded; `details` is the failure.
/// A `windows` player starts vera20k.exe from a folder of their choosing; the
/// macOS app and the Linux AppImage keep config.toml in their data folder and
/// set up the game folder on first launch (`app_bundle`).
fn shell_error_card(
    windows: bool,
    files: &crate::app::initialize::StartupFiles,
    menu_assets: bool,
    game_files: Option<&std::path::Path>,
    log_file: Option<&std::path::Path>,
    details: &str,
) -> (String, String) {
    use crate::app::initialize::StartupProblem;
    use crate::i18n::{tr, tr_fill};

    let config = files.config_file.as_deref().map_or_else(
        || "config.toml".to_owned(),
        |path| path.display().to_string(),
    );
    let log = log_file.map_or_else(
        || tr("VERA:NoLogWritten"),
        |path| path.display().to_string(),
    );
    let (title, mut text) = match &files.problem {
        Some(StartupProblem::ConfigFile) => (
            tr("VERA:ConfigUnreadable"),
            tr_fill(
                "VERA:ConfigUnreadableText",
                &[("config", &config), ("log", &log)],
            ),
        ),
        Some(StartupProblem::Settings(folder)) => {
            let folder = folder.as_deref().map_or_else(
                || tr("VERA:SettingsFolderUnknown"),
                |folder| folder.display().to_string(),
            );
            (
                tr("VERA:SettingsFolderUnusable"),
                tr_fill(
                    "VERA:SettingsFolderUnusableText",
                    &[("folder", &folder), ("config", &config), ("log", &log)],
                ),
            )
        }
        None => {
            // No archives at all means the installation is missing or wrong;
            // some archives but no menu artwork means the install is
            // incomplete. Say which, so the player is not sent looking for
            // files that are present.
            let title = tr(if menu_assets {
                "VERA:GameMenuUnavailable"
            } else {
                "VERA:GameFilesNotFound"
            });
            let advice = tr_fill(
                match windows {
                    true => "VERA:GameFilesAdviceWindows",
                    false => "VERA:GameFilesAdviceUnix",
                },
                &[("config", &config)],
            );
            let game_files = game_installation_label(game_files);
            (
                title,
                tr_fill(
                    "VERA:GameFilesNeededText",
                    &[
                        ("advice", &advice),
                        ("game_files", &game_files),
                        ("log", &log),
                    ],
                ),
            )
        }
    };
    text.push_str("\n\n");
    text.push_str(&tr_fill("VERA:StartupDetails", &[("details", &details)]));
    (title, text)
}

/// What the card and the import dialog name as the game installation.
///
/// An unconfigured launch resolves the installation to the executable's own
/// directory, which on Android is `/system/bin` (app_process). Naming that path
/// sends the player looking for game files that were never there, so on Android
/// the fallback reads as "no installation configured". The desktop builds keep
/// printing whatever path they resolved.
///
/// One function for both screens: the import dialog sits on the same card and
/// must never name a path the card behind it has stopped naming.
fn game_installation_label(game_files: Option<&std::path::Path>) -> String {
    let executable_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf));
    game_files.map_or_else(
        || crate::i18n::tr("VERA:NoGameInstallation"),
        |path| {
            if cfg!(target_os = "android") && executable_dir.as_deref() == Some(path) {
                crate::i18n::tr("VERA:NoGameInstallation")
            } else {
                path.display().to_string()
            }
        },
    )
}

impl App {
    /// Shared pre-dialog Session read: Skirmish at52E11E and LAN (including
    /// failed IPX admission) at52E37D. Preferences stay in their process
    /// caches; the original reads INI8870C0, not disk.
    /// VERA starts network selection from cold Rules so an earlier map cannot
    /// add Countries or replace the choices a peer is about to accept.
    pub(crate) fn prepare_multiplayer_settings(
        state: &mut AppState,
        mode: crate::app::match_runtime::session::SessionMode,
    ) -> bool {
        if mode.is_network()
            && let Some(owner) = state.process_assets.native_rules_mut_with_tile_cache().0
            && let Err(error) = owner.reset_for_network_match()
        {
            state.frontend.main_menu_shell_error = Some(format!(
                "Could not prepare network rules. Restart the game and try again.\n{error}"
            ));
            return false;
        }
        // VERA: setup screens start from the player's own Options switches,
        // which a previous room's replaced; a room's agreement plays its
        // host's.
        state.process_assets.use_own_personal_rules();
        //6980D6→6722F0 and475540 read the same process INI each visit and
        // may allocate a Country before the52E123 body pass.
        state
            .frontend
            .multiplayer_shell_runtime
            .read_multiplayer_settings(&mut state.process_assets);
        let main = crate::app::state::process_main_draws(&state.frontend.frontend_main_rng);
        (state.match_state.session).prepare_multiplayer_settings(&main);
        state.process_assets.refresh_multiplayer_country_bodies();
        // These are compatibility projections of the one process registry.
        // A finished world can remain resident behind the shell. Offline
        // selection keeps its native retaining view; network selection uses
        // the startup frontend projection and the freshly rebuilt Countries.
        if let Some(rules) = state.frontend.frontend_rules.as_mut() {
            state.process_assets.project_multiplayer_countries(rules);
        }
        if !mode.is_network()
            && let Some(runtime) = state.match_state.sim_runtime.write_sim(SimWrite::Rules)
        {
            state
                .process_assets
                .project_multiplayer_countries(&mut runtime.resources_mut().rules);
        }
        if let Some(rules) = state.frontend.frontend_rules.as_ref() {
            (state.frontend.skirmish_shell_state).refresh_country_catalog(rules);
        } else if let Some(runtime) = state.match_state.sim_runtime.runtime() {
            (state.frontend.skirmish_shell_state).refresh_country_catalog(&runtime.resources.rules);
        }
        state
            .frontend
            .multiplayer_shell_runtime
            .refresh_multiplayer_country_catalog(&state.process_assets);
        // The lobby's colour list comes from the same rules as the shell's
        // catalog above; its random draws and the room's claims follow it.
        let lobby_colors = state.frontend.skirmish_shell_state.lobby_colors();
        (state.frontend.multiplayer_shell_runtime).set_lobby_colors(lobby_colors);
        state.match_state.session.set_lobby_colors(lobby_colors);
        true
    }

    /// The score page `0x108` owns the result screen once its model is set
    /// and its art and the shell chrome exist; otherwise the result screen
    /// uses the asset-independent status card when retail score art is absent.
    pub(super) fn score_shell_active(state: &AppState) -> bool {
        matches!(state.frontend.screen, GameScreen::MissionResult { .. })
            && state.frontend.score_page.is_some()
            && state.frontend.score_art.is_some()
            && state.frontend.main_menu_shell_chrome.is_some()
    }

    /// Campaign victories own the native StandaloneScore dialog `0x103`.
    /// This page has no Skirmish score-art dependency.
    pub(super) fn campaign_score_shell_active(state: &AppState) -> bool {
        matches!(state.frontend.screen, GameScreen::MissionResult { .. })
            && state.frontend.campaign_score_page.is_some()
            && state.frontend.main_menu_shell_chrome.is_some()
    }

    /// `ScoreDialog__Run` `0x005C9720` after a skirmish win or loss: the side
    /// art (`ScoreArt__Load` `0x0072D730`), then dialog `0x108` with every
    /// text its init (`0x497`) sets. The exit cascade has already restored the
    /// shell size and started SCORE.
    pub(crate) fn open_score_page(
        state: &mut AppState,
        model: crate::ui::score_shell::ScoreScreenModel,
        title: String,
        detail: String,
    ) {
        use crate::ui::score_shell::{ScorePage, ScoreTexts, format_elapsed, format_game_number};
        // The page draws the main menu's chrome, dropped for the match.
        Self::ensure_main_menu_shell_chrome(state);
        let texts = ScoreTexts {
            game: format_game_number(&model, &state.csf_text("TXT_GAME")),
            time: format_elapsed(&model, &state.csf_text("TXT_TIME_FORMAT_HOURS")),
            headers: [
                "GUI:Player",
                "GUI:Kills",
                "GUI:Losses",
                "GUI:Built",
                "GUI:Score",
            ]
            .map(|key| state.csf_text(key)),
        };
        // A rules set may name its own picture for this country
        // (`[EndGameScreens]`). A name the install cannot open is not fatal:
        // the side's own picture is still a correct score page, and the model
        // then names no card, so the page keeps the retail paint order.
        let mut model = model;
        if let Some(background) = model.background.as_ref()
            && !state
                .process_assets
                .manager()
                .is_some_and(|assets| assets.contains(&background.shape))
        {
            log::warn!(
                "[EndGameScreens] {} is not installed; the side's score art stays",
                background.shape
            );
            model.background = None;
        }
        // The side's own name table still supplies what the override leaves
        // out: an art with no palette keeps the side's.
        let mut art = state.process_assets.side_score_art(model.side);
        if let Some(background) = model.background.as_ref() {
            art.multiplayer_background = Some(background.shape.as_str());
            if let Some(palette) = background.palette.as_deref() {
                art.multiplayer_palette = Some(palette);
            }
        }
        state.frontend.score_art = state.process_assets.manager().and_then(|assets| {
            crate::render::main_menu_shell_chrome::build_score_art(
                &state.renderer.gpu,
                &state.renderer.batch_renderer,
                assets,
                model.side,
                shell_layout::art_key_screen_width(state),
                art,
            )
        });
        state.frontend.campaign_score_page = None;
        state.frontend.campaign_score_art = None;
        if state.frontend.score_art.is_none() {
            log::warn!("Could not prepare the score art; the result screen falls back");
        }
        state.frontend.score_page = Some(ScorePage::open(model, &texts, Instant::now()));
        state.frontend.screen = GameScreen::MissionResult { title, detail };
        (state.frontend.shell_controller).reset_to(crate::ui::score_shell::SCORE_DIALOG);
    }

    /// Open retail Campaign StandaloneScore dialog `0x103` from the app-owned
    /// projection of the campaign result fields.
    pub(crate) fn open_campaign_score_page(
        state: &mut AppState,
        model: crate::ui::campaign_score_shell::CampaignScoreScreenModel,
        title: String,
        detail: String,
    ) {
        use crate::ui::campaign_score_shell::CampaignScorePage;

        state.frontend.score_page = None;
        state.frontend.score_art = None;
        Self::prepare_campaign_score_art(state, model.side);
        if state.frontend.campaign_score_art.is_none() {
            log::warn!(
                "Could not prepare Campaign StandaloneScore background; Continue remains available"
            );
        }
        let now = Instant::now();
        // ScoreDialog_Handle_ScoreThemeA/B (0x1002F920/0x1002F9A0) replace the
        // literal the dialog pushes with the side's par-branch theme.
        let theme = model
            .theme
            .clone()
            .unwrap_or_else(|| crate::app::match_runtime::scenario_exit::SCORE_THEME.to_owned());
        let page = CampaignScorePage::open(&model, |key| state.csf_text(key), now);
        state.frontend.campaign_score_page = Some(page);
        state.frontend.shell_first_paint_slide = None;
        state.frontend.screen = GameScreen::MissionResult { title, detail };
        state
            .frontend
            .shell_controller
            .reset_to(crate::ui::campaign_score_shell::CAMPAIGN_SCORE_DIALOG);
        // gamemd StandaloneScore dialog proc6C8FF0, WM_INIT6C9357..6C936C:
        // the theme starts here, after Victory685928's Win movie has returned.
        if let Some(assets) = state.process_assets.manager() {
            let wall_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
            let _ = state.audio.play_theme(&theme, assets, wall_ms);
        }
    }

    /// `ScoreArt__Load` (`0x0072D300`): prepare the active side's campaign
    /// score art for the standalone and Cooperative result pages.
    pub(super) fn prepare_campaign_score_art(state: &mut AppState, side: i32) {
        state.frontend.campaign_score_art = state.process_assets.manager().and_then(|assets| {
            crate::render::main_menu_shell_chrome::build_campaign_score_art(
                &state.renderer.gpu,
                &state.renderer.batch_renderer,
                assets,
                side,
                shell_layout::art_key_screen_width(state),
                state.process_assets.side_score_art(side),
            )
        });
    }

    /// Cooperative `+0xA4`'s campaign end (`0x00471420`): the side's score
    /// art again (`0x0072D300`, `ScoreArt__Load`), the empty backdrop, then
    /// dialog `0xBC7` with the texts its init writes. No theme changes.
    pub(crate) fn open_campaign_end_page(
        state: &mut AppState,
        model: crate::ui::campaign_score_shell::CampaignEndModel,
    ) {
        use crate::ui::campaign_score_shell::{CAMPAIGN_END_DIALOG, CampaignScorePage};
        Self::prepare_campaign_score_art(state, model.side);
        if state.frontend.campaign_score_art.is_none() {
            log::warn!("Could not prepare the campaign-end score art; Continue remains available");
        }
        let page = CampaignScorePage::open_campaign_end(
            model,
            |key| state.csf_label(key, ""),
            Instant::now(),
        );
        state.frontend.campaign_score_page = Some(page);
        state.frontend.shell_first_paint_slide = None;
        state.frontend.screen = GameScreen::MissionResult {
            title: String::new(),
            detail: String::new(),
        };
        (state.frontend.shell_controller).reset_to(CAMPAIGN_END_DIALOG);
    }

    /// Continue and VERA's result paging. Only Continue has help. Everywhere
    /// else the proc's common handler (`0x00622B50`, `WM_NCHITTEST` at
    /// `0x00622CCB..0x00622E83`) finds the dialog, a band (bands sit above
    /// the cells in Z-order) or another static without help and writes an
    /// empty text: every move writes the status line, which repaints it
    /// (`0x00615EF7`). Continue (`0x6D1`, result 1 at `0x005CA06C`) tears
    /// `0x108` down with its slide; the match then ends and the shell resumes.
    pub(super) fn handle_score_shell_mouse(state: &mut AppState, down: Option<bool>) {
        use crate::ui::score_shell::{self as score, NEXT_PAGE, PREVIOUS_PAGE};
        let layout = shell_layout::score_layout(state);
        let paged = state
            .frontend
            .score_page
            .as_ref()
            .is_some_and(|page| page.page_count() > 1);
        let rect_of = |id| layout.control_rect(id, paged);
        let dialog = score::SCORE_DIALOG;
        match Self::screen_page_pointer(state, dialog, score::nodes, rect_of, &[], down) {
            Some(score::CONTINUE_BUTTON) => Self::leave_shell_dialog(
                state,
                ShellSlideKind::Score,
                App::leave_mission_result_screen,
            ),
            Some(id @ (PREVIOUS_PAGE | NEXT_PAGE)) => {
                if let Some(page) = state.frontend.score_page.as_mut() {
                    page.change_page(id == NEXT_PAGE, Instant::now());
                }
            }
            _ => {}
        }
    }

    /// `0x103`'s and `0xBC7`'s pointer: hover tracks the value statics' help,
    /// and only Continue presses. Native `0x103` Continue (`0x423`, result 1)
    /// tears the dialog down with its slide before GameExit::Victory advances
    /// the campaign; `0xBC7`'s (`0x6D1`, result 1) before Main_Game returns
    /// to the shell.
    pub(super) fn handle_campaign_score_mouse(state: &mut AppState, down: Option<bool>) {
        use crate::ui::campaign_score_shell::{CampaignScoreDialog, CampaignScoreLayout};
        let (width, height) = shell_layout::shell_screen_size(state);
        let fills_window = shell_layout::fills_window(state);
        let page = state.frontend.campaign_score_page.as_ref();
        let dialog: CampaignScoreDialog = page.map_or_else(Default::default, |page| page.dialog);
        let layout = page.map_or_else(
            || CampaignScoreLayout::compute(Default::default(), width, height, 0),
            |page| page.layout(width, height, fills_window),
        );
        (state.frontend.shell_controller).ensure_active(dialog.id());
        let rect_of = |id| layout.control_rect(id);
        let nodes = || dialog.nodes();
        if Self::page_pointer(state, dialog.id(), nodes, rect_of, &[], down)
            != Some(dialog.continue_button())
        {
            return;
        }
        match dialog {
            CampaignScoreDialog::Victory => {
                // `0x423` ends the global VocHandle before the result returns.
                Self::stop_campaign_score_anim_sound(state);
                Self::leave_shell_dialog(
                    state,
                    ShellSlideKind::CampaignScore,
                    App::leave_mission_result_screen,
                );
            }
            CampaignScoreDialog::CampaignEnd => Self::leave_shell_dialog(
                state,
                ShellSlideKind::CampaignEnd,
                crate::app::cooperative_round::campaign_end_continue,
            ),
        }
    }

    /// Shared teardown for both result-screen forms: flush the deterministic log
    /// while its simulation is still alive, hand the scenario stream back to the
    /// offline shell, retaining the runtime and presentation resources.
    pub(super) fn leave_mission_result_screen(state: &mut AppState) {
        // WM_DESTROY ends 0x103's global VocHandle even when no Continue
        // command did; the stop is idempotent.
        Self::stop_campaign_score_anim_sound(state);
        // A won campaign mission continues in GameExit::Victory instead of
        // returning to the shell; read it before the teardown below.
        let campaign_victory = Self::take_campaign_victory(state);
        Self::close_finished_scenario(state);
        match campaign_victory {
            Some(victory) => Self::advance_campaign_victory(state, victory),
            None => Self::resume_shell_after_match(state),
        }
    }

    /// Retire a scenario whose outcome teardown has run: flush the
    /// deterministic log while its simulation is still alive and hand the
    /// scenario stream back to the offline shell, retaining the runtime and
    /// presentation resources.
    pub(super) fn close_finished_scenario(state: &mut AppState) {
        Self::retire_finished_scenario(state);
        Self::persist_returned_session_toggles(state);
        state.frontend.score_page = None;
        state.frontend.campaign_score_page = None;
        // 0x0072D780 releases the score art.
        state.frontend.score_art = None;
        state.frontend.campaign_score_art = None;
        state.frontend.screen = GameScreen::MainMenu;
    }

    /// The teardown every finished scenario shares, the co-op continuation
    /// included, which never reaches Main_Game's shell return.
    pub(crate) fn retire_finished_scenario(state: &mut AppState) {
        crate::app::match_runtime::sim_tick::finish_threaded_frame(state);
        // Main_Game48CEBF also retires ScenarioStarted after score Continue;
        // source: original gamemd.exe Main_Game48CCC0 (EBX=0 at48CCD4).
        if let Some(runtime) = state.match_state.sim_runtime.write_sim(SimWrite::Lifecycle) {
            runtime.simulation.retire_scenario_started();
        }
        // The accepted match owns the socket until either exit path returns
        // it to the visit's shared transport owner, including score Continue.
        crate::app::lan_service::return_transport(state);
        crate::app::match_runtime::sim_tick::flush_replay_log(state);
        Self::capture_returned_multiplayer_scenario(state);
        state.match_state.startup.clear();
        state.match_state.scenario_elapsed_clock.reset();
        state.match_state.input.zoom_level = 1.0;
        state.match_state.input.zoom_target = 1.0;
    }

    /// The main menu's chrome atlas: built at startup, dropped when a match
    /// starts (`loading::transitions`) and built again here before a shell
    /// screen reads it (the score page as it opens, the frame loop for the
    /// menu). `false` when it cannot be built: the error card
    /// (`main_menu_shell_error`) keeps the words a startup failure gets, and
    /// no frame tries again.
    pub(crate) fn ensure_main_menu_shell_chrome(state: &mut AppState) -> bool {
        if state.frontend.main_menu_shell_chrome.is_some() {
            return true;
        }
        if state.frontend.main_menu_shell_error.is_some() {
            return false;
        }
        let Some(assets) = state.process_assets.manager() else {
            log::warn!(
                "Could not prepare the main menu shell chrome: process asset manager is unavailable"
            );
            return false;
        };
        let started = crate::util::time::Instant::now();
        let built = crate::render::main_menu_shell_chrome::build_main_menu_shell_chrome_atlas(
            &state.renderer.gpu,
            &state.renderer.batch_renderer,
            assets,
        );
        match built {
            Ok(atlas) => {
                log::info!(
                    "Main-menu shell chrome built in {:.0} ms",
                    started.elapsed().as_secs_f64() * 1e3
                );
                state.frontend.main_menu_shell_chrome = Some(atlas);
                true
            }
            Err(error) => {
                log::warn!("{error:#}");
                state.frontend.main_menu_shell_error =
                    Some(super::initialize::describe_startup_error(&error));
                false
            }
        }
    }

    /// PrepareSession re-enters the active mode's shell: LAN starts a new BB,
    /// WOL its lobby 0x113, Skirmish a new0x102. The session keeps the
    /// admitted network return owner.
    pub(crate) fn resume_shell_after_match(state: &mut AppState) {
        if state.frontend.external_launch {
            // A relay launch first reports its result to the match service;
            // its live service starts the same quit once that settles.
            if !crate::app::shell_wol::end_spawned_match(state) {
                Self::start_quit_cascade(state);
            }
            return;
        }
        if state.match_state.session.network_return_context().is_some() {
            crate::app::shell_network_setup::return_after_match(state);
            return;
        }
        if state.frontend.shell_route == ShellRoute::Skirmish {
            Self::enter_native_skirmish_from_single_player(state);
        }
    }

    /// `0x497` enables Load Saved Game when the scan `0x00559C20` finds a
    /// save the list reader accepts; the list `0x005596A0` uses the same
    /// reader, so an enabled button always opens a non-empty list.
    fn refresh_single_player_load_state(state: &mut AppState) {
        state
            .frontend
            .single_player_shell_state
            .load_saved_game_enabled = state.persistence.repository.has_browser_entry();
    }

    pub(super) fn open_single_player_shell(state: &mut AppState) {
        // Native destroys 0xE2 (including child 0x71A) before constructing
        // 0x100. Invalidate at the route edge rather than waiting for a paint:
        // a queued Back/Escape can otherwise return to 0xE2 before 0x100 draws
        // and incorrectly preserve the old main-menu movie timeline.
        Self::destroy_current_shell_dialog(state);
        state.frontend.shell_route = ShellRoute::SinglePlayer;
        // A new 0x100 instance starts with no press or hover.
        state
            .frontend
            .shell_controller
            .reset_to(crate::ui::single_player_shell::SINGLE_PLAYER_PAGE.dialog);
        Self::refresh_single_player_load_state(state);
    }

    pub(super) fn close_single_player_shell(state: &mut AppState) {
        // Result 0x12 destroys 0x100 before the main-menu loop constructs a new
        // 0xE2. Clear immediately so a same-event-loop round trip cannot reuse
        // the source dialog's movie session.
        Self::destroy_current_shell_dialog(state);
        state.frontend.shell_route = ShellRoute::MainMenu;
    }

    /// A new `0x102` from Single Player's Skirmish or, after a Skirmish game,
    /// from PrepareSession: GameMode stays 5, so state `0x10` re-reads the
    /// settings and runs the Skirmish runner again (`0x0052DB09..0x0052DB24`,
    /// `0x0052E119`, `0x0052E168` -> `0x006AE2C0`). Its Back returns to
    /// Single Player either way (GameMode 0, state 1, `0x0052E175`).
    pub(super) fn enter_native_skirmish_from_single_player(state: &mut AppState) {
        if state.process_assets.uses_client_catalog()
            && (state.frontend.skirmish_modes.is_empty()
                || state.frontend.scenario_catalog.shell_maps().is_empty())
        {
            Self::open_shell_notice(
                state,
                "No playable skirmish maps were found in this mod's client catalog. Check that the full mod folder was extracted.".into(),
            );
            return;
        }
        if !Self::prepare_multiplayer_settings(
            state,
            crate::app::match_runtime::session::SessionMode::Skirmish,
        ) {
            return;
        }
        // Prepare dialog 0x102 from the process-lifetime MIX list before
        // destroying its 0x100 source. Active YR's FUN_00534E50 registers the
        // neutral pair on that shared list before the shell SHPs are loaded.
        if !Self::ensure_skirmish_shell_chrome(state) {
            log::warn!("Skirmish shell chrome unavailable; retaining the Single Player shell");
            return;
        }

        // Native destroys dialog 0x100 and its child 0x71A movie handle before
        // constructing 0x102. Drop the hidden Rust session as well so returning
        // to 0x100 cannot continue the pre-Skirmish RA2TS timeline.
        crate::app::frontend::main_menu_shell_render::clear_ra2ts_movie_session(state);
        state.frontend.shell_route = ShellRoute::Skirmish;
        // A fresh `0x102`: its widgets build on the first sync.
        (state.frontend.shell_controller).reset_to(crate::ui::skirmish_shell::page::DIALOG);
        // A new 0x102 creates its kind-1 statics hidden (0x0060A5B0).
        state.frontend.skirmish_shell_state.statics.reset();
        // Native constructs 0x102 fresh on every visit, and
        // `SkirmishDialog__OnSetup497` hands its controls the Session values that
        // `prepare_multiplayer_settings` just refreshed above. The retained Rust
        // controls follow that same order, so a durable record changed since the
        // last visit reaches the page instead of the page keeping its old face.
        state.frontend.multiplayer_shell_runtime.open_skirmish_page(
            &mut state.frontend.skirmish_shell_state,
            &state.frontend.scenario_catalog,
            &state.frontend.skirmish_modes,
        );
        // The skirmish dialog (0x102) slides its controls in on first paint like
        // every shell dialog; the per-frame slide trigger starts that wave once
        // the skirmish shell becomes the showing screen. Clear any stale wave
        // from the source shell here so the trigger restarts cleanly.
        state.frontend.shell_first_paint_slide = None;
    }

    pub(super) fn render_shell_error(
        state: &mut AppState,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) -> Result<()> {
        let error = state
            .frontend
            .main_menu_shell_error
            .get_or_insert_with(|| crate::i18n::tr("VERA:MenuResourcesMissing"))
            .clone();
        let game_files = state
            .platform
            .game_config
            .as_ref()
            .map(|config| config.paths.ra2_dir.clone());
        let (title, text) =
            if crate::ui::asset_import::AVAILABLE && state.frontend.asset_import.is_open() {
                // Same label as the card behind it: the dialog must not name a
                // fallback path the card has just stopped naming.
                let game_files_text = game_installation_label(game_files.as_deref());
                (
                    state.frontend.asset_import.title(),
                    state.frontend.asset_import.body(&game_files_text, ""),
                )
            } else {
                let (title, mut text) = shell_error_card(
                    cfg!(target_os = "windows"),
                    &state.frontend.startup_files,
                    state.process_assets.manager().is_some(),
                    game_files.as_deref(),
                    crate::util::logging::log_file(),
                    &error,
                );
                // The next start is without the mod (`mods::finish_start`).
                if let Some(active) = &state.frontend.mods.active {
                    text.push_str("\n\n");
                    text.push_str(&crate::app::mods::start_failed_text(&active.title));
                }
                (title, text)
            };
        crate::app::frontend::status_screen::render(
            state,
            encoder,
            view,
            &title,
            &text,
            crate::app::frontend::status_screen::shell_error_buttons(state),
        );
        Ok(())
    }

    /// While the Developer page's String Keys box is ticked, reload a language
    /// file rewritten in the settings folder or the checkout's `src/i18n`, so
    /// its texts (and their keys) follow without a restart, as after a pick
    /// in the Language list ([`Self::use_language`]). Returns the next check,
    /// keeping the event loop awake for it even on a still screen.
    pub(super) fn poll_language_files(state: &mut AppState) -> Option<Instant> {
        if !crate::i18n::string_keys_enabled() {
            return None;
        }
        let settings_dir = state
            .platform
            .game_config
            .as_ref()
            .map(|config| config.paths.settings_dir().to_path_buf());
        if state.frontend.language_files.poll(settings_dir.as_deref()) {
            state.refresh_string_key_labels();
            state.platform.window.request_redraw();
        }
        Some(Instant::now() + Duration::from_secs(1))
    }

    /// Construct a fresh launcher Options primary from the current retained
    /// profile, the window's full-screen state, live CSF table, and frozen
    /// process-start common audio gate. Nothing is applied here.
    pub(crate) fn open_launcher_options_dialog(state: &mut AppState) {
        // State 5 destroys `0xE2` (and its RA2TS static) before Options runs;
        // closing Options creates a new `0xE2` with its own entry slide.
        Self::destroy_current_shell_dialog(state);
        Self::ensure_skirmish_shell_chrome(state);
        if state.frontend.skirmish_shell_chrome.is_none() {
            state.frontend.main_menu_shell_error =
                Some(crate::i18n::tr("VERA:OptionsArtworkMissing"));
            state.platform.window.request_redraw();
            return;
        }
        let mut dialog = Self::launcher_options_from_state(state);
        dialog.select_category(
            state
                .frontend
                .options_reopen_at
                .take()
                .unwrap_or(crate::ui::main_menu_dialogs::options::Category::LauncherMain),
        );
        state.frontend.options_dialog = Some(dialog);
    }

    /// The Options values as they stand now.
    pub(crate) fn launcher_options_from_state(
        state: &AppState,
    ) -> crate::ui::main_menu_dialogs::OptionsDialogState {
        use crate::app::persistence::options::launcher::launcher_dialog_from_profile;

        let labels = Self::launcher_options_labels(state);
        // The Display list is VERA's own: it reads the screens attached now, so
        // a screen plugged in or removed since the last launch shows correctly.
        let displays = crate::app::display_target::current_displays(&state.platform.window)
            .iter()
            .map(|display| display.choice().clone())
            .collect();
        let mut dialog = launcher_dialog_from_profile(
            &state.persistence.options_profile,
            labels,
            state.platform.window.fullscreen().is_some(),
            state.renderer.performance_overlay.is_visible(),
            state.audio.launcher_audio_available,
            displays,
        );
        dialog.set_languages(crate::i18n::choices(), crate::i18n::selected_language());
        let launch = state.platform.launch_window_size;
        dialog.set_launch_window_size((launch.width, launch.height));
        dialog
    }

    /// The Options captions the string table names, in the language in use.
    pub(crate) fn launcher_options_labels(
        state: &AppState,
    ) -> crate::ui::main_menu_dialogs::options::LauncherOptionsLabels {
        let mut labels =
            crate::ui::main_menu_dialogs::options::LauncherOptionsLabels::resolve(&|key| {
                state
                    .process_assets
                    .csf
                    .as_ref()
                    .and_then(|csf| csf.get(key))
                    .map(str::to_owned)
            });
        // VERA's Esc menu opens Options over a page: the button returns there.
        if state.frontend.shell_route != ShellRoute::MainMenu {
            labels.set("GUI:MainMenu", state.csf_label("GUI:Back", "Back"));
        }
        labels
    }

    /// The sound of a shell control's cue (`ui::shell::page`): an owner-draw
    /// button's GUIMainButtonSound (`Rules+0x188`), which the button procedure
    /// `0x00612B70` plays on every admitted press (`0x0061374B..0x00613771`)
    /// in shell and in-game dialogs alike, the checkbox and combo sounds, and
    /// a list row's GenericClick.
    /// RESIDUAL: a frame-type-0 (sliced PCX) text button also plays
    /// GenericClick when it paints pressed after an unpressed type-0 paint
    /// (`0x0061323C..0x00613289`); that paint-time cue is not ported. Every
    /// in-game dialog button is type 2 or 3 (`0x0060A400`), so it is a shell
    /// difference only.
    pub(crate) fn play_page_cue(state: &mut AppState, cue: PageCue) {
        Self::play_rules_sound(state, |general| match cue {
            PageCue::Button => general.gui_main_button_sound.as_deref(),
            PageCue::ListClick => general.generic_click_sound.as_deref(),
            PageCue::Checkbox => general.gui_checkbox_sound.as_deref(),
            PageCue::ComboOpen => general.gui_combo_open_sound.as_deref(),
            PageCue::ComboClose => general.gui_combo_close_sound.as_deref(),
        });
    }

    /// Plays the rules' sound `pick` names; nothing while it is unset.
    fn play_rules_sound(
        state: &mut AppState,
        pick: impl FnOnce(&crate::rules::ruleset::GeneralRules) -> Option<&str>,
    ) {
        let sound = state
            .rules()
            .and_then(|rules| pick(&rules.general))
            .map(str::to_owned);
        Self::play_shell_ui_sound_by_id(state, sound.as_deref());
    }

    pub(crate) fn native_launcher_options_active(state: &AppState) -> bool {
        state.frontend.screen == GameScreen::MainMenu
            && state.frontend.options_dialog.is_some()
            && state.frontend.skirmish_shell_chrome.is_some()
    }

    /// Play an Options dialog's cues, then run its preview events in order;
    /// true when one picked a language, after which the dialog's captions
    /// read again ([`Self::launcher_options_labels`]).
    pub(crate) fn run_launcher_options_events(
        state: &mut AppState,
        cues: Vec<PageCue>,
        events: Vec<crate::ui::main_menu_dialogs::options::LauncherOptionsEvent>,
    ) -> bool {
        use crate::ui::main_menu_dialogs::options::LauncherOptionsEvent;
        for cue in cues {
            Self::play_page_cue(state, cue);
        }
        let language = events
            .iter()
            .any(|event| matches!(event, LauncherOptionsEvent::LanguageSelected(_)));
        for event in events {
            crate::app::persistence::options::launcher::dispatch_launcher_preview_event(
                state, event,
            );
        }
        language
    }

    /// What the dialog's controls did: its cues and preview events, then a
    /// parent result or Check for Updates; profile and audio authority remain
    /// in their owners.
    fn dispatch_launcher_options_output(
        state: &mut AppState,
        mut dialog: crate::ui::main_menu_dialogs::OptionsDialogState,
    ) {
        let output = dialog.drain_output();
        if Self::run_launcher_options_events(state, output.cues, output.events) {
            dialog.set_labels(Self::launcher_options_labels(state));
        }
        match output.result {
            // Retail slides D5 out before its Keyboard page (0x0055FD06);
            // VERA's Options window is its own layout, and its Keyboard page
            // opens in place like its other pages: no slide, no cue
            // (`shell_transition::moves_in_place`).
            Some(LauncherParentResult::Keyboard) => {
                state.frontend.options_dialog = Some(dialog);
                Self::commit_launcher_options_result(state, LauncherParentResult::Keyboard);
            }
            // 0x0055FD06 tears D5 down before every other continuation,
            // including the final Main Menu return. VERA's Mods leaves the
            // same way.
            Some(result @ (LauncherParentResult::Back | LauncherParentResult::Mods)) => {
                state.frontend.options_dialog = Some(dialog);
                Self::leave_shell_dialog(state, ShellSlideKind::Options, move |state| {
                    App::commit_launcher_options_result(state, result)
                });
            }
            Some(result) => {
                crate::app::persistence::options::launcher::dispatch_launcher_parent_result(
                    state, dialog, result, None,
                );
            }
            None => {
                state.frontend.options_dialog = Some(dialog);
                #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
                if output.check_for_updates {
                    Self::leave_shell_dialog(state, ShellSlideKind::Options, |state| {
                        // Options closes as through Back, keeping and saving
                        // its values; the main menu returns after the
                        // updater's last box.
                        Self::commit_launcher_options_result(state, LauncherParentResult::Back);
                        crate::app::updater::open_check(state);
                    });
                }
            }
        }
    }

    /// `0xD5`'s teardown slide has run: commit the controls (`0x0055FAA0`),
    /// then write the profile and build `0xE2` (Main Menu, `0x005FAD10`), or
    /// run the Keyboard page or VERA's Mods page.
    pub(super) fn commit_launcher_options_result(
        state: &mut AppState,
        result: crate::ui::main_menu_dialogs::options::LauncherParentResult,
    ) {
        // The key page and VERA's Mods page are children of Options: closing
        // one reopens the dialog on the page the player left. Back and
        // Terminal end the dialog for good.
        state.frontend.options_reopen_at = match result {
            LauncherParentResult::Keyboard | LauncherParentResult::Mods => state
                .frontend
                .options_dialog
                .as_ref()
                .map(|dialog| dialog.category()),
            _ => None,
        };
        let Some(dialog) = state.frontend.options_dialog.take() else {
            return;
        };
        crate::app::persistence::options::launcher::dispatch_launcher_parent_result(
            state, dialog, result, None,
        );
    }

    /// The in-game menu's Options page has closed: the controls the player
    /// moved commit as the main menu's do, and the profile is written once.
    pub(crate) fn commit_in_game_options_page(
        state: &mut AppState,
        dialog: crate::ui::main_menu_dialogs::OptionsDialogState,
        opened: crate::ui::main_menu_dialogs::options::LauncherOptionsPacked,
    ) {
        crate::app::persistence::options::launcher::dispatch_launcher_parent_result(
            state,
            dialog,
            LauncherParentResult::Back,
            Some(opened),
        );
    }

    pub(crate) fn handle_launcher_options_mouse(state: &mut AppState, down: Option<bool>) {
        if down.is_none() {
            // Every hover message repaints the status line (0x00615EF7).
            state.frontend.shell_status_line.repaint();
        }
        let Some(mut dialog) = state.frontend.options_dialog.take() else {
            return;
        };
        let (x, y) = (
            state.match_state.input.cursor_x as i32,
            state.match_state.input.cursor_y as i32,
        );
        let layout = shell_layout::launcher_options_layout(state, dialog.category());
        match down {
            Some(true) => dialog.shell_mouse_down(x, y, &layout),
            Some(false) => dialog.shell_mouse_up(x, y, &layout),
            None => dialog.shell_mouse_move(x, y, &layout),
        }
        let next = dialog.category();
        if next != layout.category
            && crate::ui::main_menu_dialogs::options::shell::launcher_slide_column(next).is_some()
        {
            dialog.select_category(layout.category);
            Self::dispatch_launcher_options_output(state, dialog);
            Self::leave_shell_dialog(state, ShellSlideKind::Options, move |state| {
                if let Some(dialog) = state.frontend.options_dialog.as_mut() {
                    dialog.select_category(next);
                    crate::app::frontend::shell_transition::invalidate_main_menu_dialog_instance(
                        state,
                    );
                }
            });
            return;
        }
        Self::dispatch_launcher_options_output(state, dialog);
    }

    /// Pump-terminal completion enters the same always-apply parent transaction
    /// as Back, then writes once before the caller performs unrelated exit work.
    pub(crate) fn close_launcher_options_terminal(state: &mut AppState) {
        let Some(dialog) = state.frontend.options_dialog.take() else {
            return;
        };
        crate::app::persistence::options::launcher::dispatch_launcher_parent_result(
            state,
            dialog,
            LauncherParentResult::Terminal,
            None,
        );
    }

    /// One pointer message (`down`: `None` moves) on a family page, with its
    /// nodes on their layout rects and its runtime-disabled controls; the
    /// activated control.
    pub(super) fn family_page_pointer(
        state: &mut AppState,
        page: FamilyPage,
        down: Option<bool>,
    ) -> Option<u16> {
        let layout = page.layout(state);
        let enabled = page.enabled(state);
        Self::screen_page_pointer(
            state,
            page.dialog(),
            || page.nodes(),
            |id| layout.button_rect(id),
            enabled.as_slice(),
            down,
        )
    }

    /// [`Self::page_pointer`] on the screen's own `dialog`: binds the
    /// controller to it (`ensure_active` resets only on a dialog change, so a
    /// gesture's press survives to its release), and every hover message
    /// repaints status line `0x695` (`0x00615EF7`).
    pub(super) fn screen_page_pointer(
        state: &mut AppState,
        dialog: crate::ui::shell::descriptor::DialogResourceId,
        nodes: impl FnOnce() -> vera20k_gui::NodeSpec,
        rect_of: impl Fn(u16) -> Option<crate::ui::shell::geom::RectPx>,
        enabled: &[(u16, bool)],
        down: Option<bool>,
    ) -> Option<u16> {
        state.frontend.shell_controller.ensure_active(dialog);
        let activated = Self::page_pointer(state, dialog, nodes, rect_of, enabled, down);
        if down.is_none() {
            state.frontend.shell_status_line.repaint();
        }
        activated
    }

    /// One pointer message (`down`: `None` moves) on the top dialog `dialog`,
    /// whose controls are `nodes`: builds them once, keeps them on `rect_of`
    /// with the `enabled` overrides, routes the pointer and plays each
    /// control's cue (an enabled button's GUIMainButtonSound on mouse DOWN,
    /// `0x0061374B`). Returns the activated control.
    pub(crate) fn page_pointer(
        state: &mut AppState,
        dialog: crate::ui::shell::descriptor::DialogResourceId,
        nodes: impl FnOnce() -> vera20k_gui::NodeSpec,
        rect_of: impl Fn(u16) -> Option<crate::ui::shell::geom::RectPx>,
        enabled: &[(u16, bool)],
        down: Option<bool>,
    ) -> Option<u16> {
        let (x, y) = state.match_state.input.cursor_px();
        let event = crate::app::input::dialog_page::event_at(x, y, down);
        Self::page_input(state, dialog, nodes, rect_of, enabled, event)
    }

    /// [`Self::page_pointer`] for any input: a key reaches the dialog's
    /// `Default`/`Cancel` buttons.
    pub(crate) fn page_input(
        state: &mut AppState,
        dialog: crate::ui::shell::descriptor::DialogResourceId,
        nodes: impl FnOnce() -> vera20k_gui::NodeSpec,
        rect_of: impl Fn(u16) -> Option<crate::ui::shell::geom::RectPx>,
        enabled: &[(u16, bool)],
        event: vera20k_gui::InputEvent,
    ) -> Option<u16> {
        Self::page_input_cued(state, dialog, nodes, rect_of, enabled, event, true)
    }

    /// [`Self::page_input`]; a list row's GenericClick plays only with
    /// `list_click`.
    pub(crate) fn page_input_cued(
        state: &mut AppState,
        dialog: crate::ui::shell::descriptor::DialogResourceId,
        nodes: impl FnOnce() -> vera20k_gui::NodeSpec,
        rect_of: impl Fn(u16) -> Option<crate::ui::shell::geom::RectPx>,
        enabled: &[(u16, bool)],
        event: vera20k_gui::InputEvent,
        list_click: bool,
    ) -> Option<u16> {
        let controller = &mut state.frontend.shell_controller;
        crate::ui::shell::menu_page::sync_nodes(controller, dialog, nodes, rect_of, enabled);
        controller.on_input(event);
        let events = Self::shell_page_events(state, list_click);
        events.into_iter().rev().find_map(|event| match event {
            crate::ui::shell::page::PageEvent::Activated(id) => Some(id),
            _ => None,
        })
    }

    /// The shell controller's events of the last input, in order, after
    /// [`Self::play_page_cues`].
    pub(crate) fn shell_page_events(
        state: &mut AppState,
        list_click: bool,
    ) -> Vec<crate::ui::shell::page::PageEvent> {
        let controller = &mut state.frontend.shell_controller;
        let raw = controller.drain_events();
        let events = crate::ui::shell::page::page_events(controller.gui(), raw);
        Self::play_page_cues(state, events, list_click)
    }

    /// Plays `events`' control cues in order and returns the other events. A
    /// list row's GenericClick plays here only with `list_click`; otherwise
    /// the page plays it where its list proc does.
    pub(crate) fn play_page_cues(
        state: &mut AppState,
        events: Vec<crate::ui::shell::page::PageEvent>,
        list_click: bool,
    ) -> Vec<crate::ui::shell::page::PageEvent> {
        use crate::ui::shell::page::PageEvent;
        let mut rest = Vec::with_capacity(events.len());
        for event in events {
            match event {
                PageEvent::Cue(PageCue::ListClick) if !list_click => {}
                PageEvent::Cue(cue) => Self::play_page_cue(state, cue),
                event => rest.push(event),
            }
        }
        rest
    }

    /// A pointer message on the main menu or a menu page, and the dialog
    /// result its activated button writes.
    pub(super) fn handle_family_page_mouse(
        state: &mut AppState,
        page: FamilyPage,
        down: Option<bool>,
    ) {
        let Some(id) = Self::family_page_pointer(state, page, down) else {
            return;
        };
        match page {
            FamilyPage::MainMenu => {
                use crate::ui::main_menu_shell::{
                    MainMenuControlId, MainMenuShellAction as Action,
                };
                // Every result destroys 0xE2 (Exit's confirmation is state 6,
                // shown over the empty shell backdrop).
                if let Some(action) = MainMenuControlId::from_resource_id(id)
                    .map(crate::ui::main_menu_shell::action_for_control)
                    && !matches!(action, Action::None)
                {
                    Self::leave_shell_dialog(state, ShellSlideKind::MainMenu, move |state| {
                        App::handle_main_menu_shell_action(state, action)
                    })
                }
            }
            // States 0x12, 8, 9 and 0xB destroy 0x100.
            FamilyPage::SinglePlayer => {
                if let Some(action) =
                    crate::ui::single_player_shell::SinglePlayerControlId::from_resource_id(id)
                        .map(crate::ui::single_player_shell::action_for_control)
                {
                    Self::leave_shell_dialog(state, ShellSlideKind::SinglePlayer, move |state| {
                        App::handle_single_player_shell_action(state, action)
                    });
                }
            }
            FamilyPage::MoviesAndCredits => {
                if let Some(action) = crate::ui::movies_credits_shell::action_for_control(id) {
                    Self::leave_shell_dialog(
                        state,
                        ShellSlideKind::MoviesAndCredits,
                        move |state| App::handle_movies_credits_action(state, action),
                    );
                }
            }
            // `0x94` routes its pointer itself (`shell_campaign`).
            FamilyPage::Campaign => {}
        }
    }

    pub(super) fn handle_main_menu_shell_mouse(state: &mut AppState, down: Option<bool>) {
        Self::handle_family_page_mouse(state, FamilyPage::MainMenu, down);
    }

    /// A press (`down`) or release on the open box, after the update offer's
    /// change list and the Esc menu's settings (a press either takes is
    /// theirs). The box (`0x005D3490`, proc `0x005D36A0`) subclasses its
    /// owner-draw OK/Cancel to `0x00612B70`, which plays GUIMainButtonSound
    /// on the press (`0x0061374B`).
    pub(super) fn handle_shell_message_box_mouse(state: &mut AppState, down: bool) {
        let Some(shell_box) = state
            .frontend
            .shell_message_box
            .as_ref()
            .map(|b| b.shell_box.clone())
        else {
            return;
        };
        if state.frontend.shell_controller.top_id() != Some(shell_box.dialog) {
            return;
        }
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        crate::app::updater::notes::sync(state);
        if Self::shell_menu_pointer(state, Some(down)) && down {
            return;
        }
        #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
        super::shell_mod_import::sync(state);
        let parts = shell_layout::shell_box_parts(state, &shell_box);
        let id = Self::shell_box_pointer(state, &shell_box, &parts, Some(down));
        // Add Mod's More and reorder buttons act inside the box.
        #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
        if let Some(id) = id
            && super::shell_mod_import::is_preview_button(id)
        {
            Self::activate_mod_import_button(state, id);
            return;
        }
        if let Some(answer) = id.and_then(|id| shell_box.answer(id)) {
            Self::resolve_shell_message_box(state, answer);
            return;
        }
        #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
        super::shell_mod_import::follow_list(state);
    }

    /// A pointer message (`down`, or `None` for a move) on `shell_box`, open
    /// on the controller with `parts`: its owner-draw buttons (`0x00612B70`)
    /// play GUIMainButtonSound on the press. The button released on, if any.
    pub(crate) fn shell_box_pointer(
        state: &mut AppState,
        shell_box: &crate::ui::shell::modal::ShellBox,
        parts: &crate::ui::shell::modal::BoxParts,
        down: Option<bool>,
    ) -> Option<u16> {
        let (x, y) = state.match_state.input.cursor_px();
        let event = crate::app::input::dialog_page::event_at(x, y, down);
        Self::shell_box_input(state, shell_box, parts, event)
    }

    /// [`Self::shell_box_pointer`] for any input: Enter and Escape reach
    /// the box's hidden IDOK ([`crate::ui::shell::modal::ShellBox::nodes`]).
    pub(crate) fn shell_box_input(
        state: &mut AppState,
        shell_box: &crate::ui::shell::modal::ShellBox,
        parts: &crate::ui::shell::modal::BoxParts,
        event: vera20k_gui::InputEvent,
    ) -> Option<u16> {
        let nodes = || shell_box.nodes();
        let rect_of = |id| parts.rect_of(id);
        let enabled = shell_box.enabled();
        // Add Mod's parts list selects a row without a sound.
        let list_click = !state
            .frontend
            .shell_message_box
            .as_ref()
            .is_some_and(|message_box| message_box.is_mod_import());
        let dialog = shell_box.dialog;
        Self::page_input_cued(state, dialog, nodes, rect_of, &enabled, event, list_click)
    }

    /// The box's answer (the button's index: callback `0x005D36A0` writes 0
    /// for `0x5AE` and 1 for control 2 or IDOK) returns to its caller once
    /// the box has closed through the controller pop (D-B3), so mouse and Esc
    /// converge on the same teardown.
    pub(super) fn resolve_shell_message_box(state: &mut AppState, answer: usize) {
        let Some(message_box) = state.frontend.shell_message_box.take() else {
            return;
        };
        Self::close_shell_message_box_from_controller(state);
        let yes = answer == 0;
        match message_box.purpose {
            Purpose::ShellMenu(_) => Self::answer_shell_menu(state, answer),
            Purpose::ExitGame if yes => Self::confirm_quit(state),
            Purpose::ExitGame | Purpose::Notice => state.platform.window.request_redraw(),
            Purpose::CampaignReplay(retry) => Self::answer_campaign_replay(state, retry, answer),
            Purpose::CampaignVictory(victory) => Self::advance_campaign_victory(state, *victory),
            Purpose::AuthoredCampaignResult(choice) => {
                crate::app::match_runtime::stage_result::answer(state, choice, answer)
            }
            #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
            Purpose::UpdateOffer(_) => crate::app::updater::answer_offer(state, answer),
            #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
            Purpose::UpdateProgress => crate::app::updater::cancel_download(state),
            #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
            Purpose::UpdateCheck => crate::app::updater::cancel_check(state),
            Purpose::ModRestart(folder) => Self::answer_mod_restart(state, folder, yes),
            Purpose::ModFiles(which) => Self::answer_mod_files(state, which, yes),
            Purpose::ModMade(dir) => Self::answer_mod_made(state, &dir, yes),
            #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
            Purpose::ModImport(dialog) => Self::answer_mod_import(state, *dialog, answer),
        }
    }

    /// Exit Game's OK: dismiss the confirmation first, persist settings to
    /// VERA.INI, then run the graceful cascade (music fade → trailing-voice
    /// wait → hard stop → exit) via render_frame instead of exiting
    /// immediately. The updater quits the same way once its update is ready.
    pub(super) fn confirm_quit(state: &mut AppState) {
        Self::close_shell_message_box_from_controller(state);
        Self::persist_settings_on_quit(state);
        Self::start_quit_cascade(state);
    }

    /// What ending the program from any shell page saves first.
    pub(super) fn prepare_shell_terminal_exit(state: &mut AppState) {
        // Active YR treats a terminated launcher Options pump as an
        // always-apply final result: Apply -> destroy -> one write.
        // Complete that transaction before unrelated shell teardown.
        crate::app::input::keyboard::prepare_terminal_exit(state);
        Self::close_launcher_options_terminal(state);
        crate::app::input::pause_menu::on_match_end(state);
        if crate::app::input::precedence::route_page(state)
            == crate::app::input::precedence::ScreenPage::Skirmish
        {
            // A terminal exit has no Start/Back transaction, so pack the
            // page's current controls into the durable record first. The
            // pack spends no `Scenario` draw: Random markers keep the
            // value already cached behind them.
            state.frontend.multiplayer_shell_runtime.pack_durable_state(
                &state.frontend.skirmish_shell_state,
                state.frontend.scenario_catalog.shell_maps(),
                &state.frontend.skirmish_modes,
            );
            Self::teardown_skirmish_shell(state);
            state.frontend.multiplayer_shell_runtime.persist_snapshot();
        }
    }

    /// Play the shell first-paint slide-in cue ([AudioVisual] GUIMoveInSound,
    /// stock `MenuSlideIn`), once at the start of each allow-listed shell
    /// dialog's controls-reveal slide. A no-op when the key is empty/unset.
    pub(crate) fn play_shell_slide_in_sound(state: &mut AppState) {
        Self::play_rules_sound(state, |general| general.gui_move_in_sound.as_deref());
    }

    /// Leave the showing family dialog `kind` with the result `then`: its
    /// teardown (`0x00622720`) slides the buttons out first (`0x00608070`),
    /// and the result runs when the slide has ended. A dialog that is not
    /// showing steady commits at once; a dialog already sliding out keeps its
    /// result.
    pub(super) fn leave_shell_dialog(
        state: &mut AppState,
        kind: crate::app::frontend::shell_transition::ShellSlideKind,
        then: impl FnOnce(&mut AppState) + 'static,
    ) {
        Self::begin_leaving_shell_dialog(state, kind, false, Box::new(then));
    }

    /// Leave network setup `dialog` for the agreed start: the only result the
    /// covered room commits without a presented slide.
    pub(super) fn leave_shell_dialog_for_network_start(
        state: &mut AppState,
        dialog: crate::ui::multiplayer_setup::SetupDialog,
        agreed_seed: u32,
    ) {
        Self::begin_leaving_shell_dialog(
            state,
            ShellSlideKind::NetworkSetup(dialog),
            true,
            Box::new(move |state| {
                crate::app::shell_network_setup::commit_start(state, agreed_seed)
            }),
        );
    }

    fn begin_leaving_shell_dialog(
        state: &mut AppState,
        kind: crate::app::frontend::shell_transition::ShellSlideKind,
        network_start: bool,
        then: crate::app::frontend::shell_transition::ShellExitThen,
    ) {
        use crate::app::frontend::shell_transition::{ShellExitStart, begin_shell_exit};
        match begin_shell_exit(state, kind, network_start, then) {
            // GUIMoveOutSound (stock `MenuSlideOut`) as `0x00608070` starts.
            ShellExitStart::Sliding => {
                Self::play_rules_sound(state, |general| general.gui_move_out_sound.as_deref());
            }
            ShellExitStart::Immediate(then) => then(state),
            ShellExitStart::AlreadyLeaving => {}
        }
    }

    /// Run a finished teardown slide's result.
    pub(super) fn drive_shell_exit(state: &mut AppState) {
        if let Some(then) =
            crate::app::frontend::shell_transition::advance_shell_exit(state, Instant::now())
        {
            then(state);
        }
    }

    /// `ShellButtonSlideSound`, which the slide engine `0x006071E0` plays as
    /// a slide-in ends, before the dialog's SHOW completion `0x4EC`
    /// (`0x00607F46..0x00607F9B`); a slide-out ends with `0x4ED` and no
    /// sound. Stock rules leave the key empty, so this is silent there.
    pub(crate) fn play_shell_slide_completion_sound(state: &mut AppState) {
        Self::play_rules_sound(state, |general| general.shell_button_slide_sound.as_deref());
    }

    pub(super) fn maintain_main_menu_intro(state: &mut AppState) {
        if state.frontend.screen != GameScreen::MainMenu || state.frontend.quit_cascade.is_some() {
            return;
        }
        // Play_Movie runs no audio pump; the paused theme resumes afterwards.
        if state.frontend.fullscreen_movie.is_some() {
            return;
        }
        let now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
        // The common service pumps Theme for Credits and WOL too, without
        // these routes running the menu loop's Play_Song(INTRO).
        if state.frontend.credits_roll.is_some() {
            return;
        }
        // GameExit::Defeat6860E5's replay prompt is still inside Main_Game.
        // Its modal pump services the retained Theme; PrepareSession52D9BF
        // selects INTRO only after No leaves the game. Yes uses StartScenario.
        // Source: original gamemd.exe 6860EA..686296, 48CFAA, 52D9A0.
        if matches!(
            state
                .frontend
                .shell_message_box
                .as_ref()
                .map(|message| &message.purpose),
            Some(
                Purpose::CampaignReplay(_)
                    | Purpose::CampaignVictory(_)
                    | Purpose::AuthoredCampaignResult(_)
            )
        ) {
            return;
        }
        // WOL_Main's mode loop runs the whole online visit, its game rooms
        // included, so the lobby music keeps playing there; only leaving the
        // visit returns to PrepareSession's INTRO (0x0052E28E). VERA's room
        // uses the NetworkSetup route, so ask for the visit, not the route.
        if state.frontend.wol.is_some() {
            return;
        }
        if let Some(assets) = state.process_assets.manager() {
            state.audio.maintain_main_menu_theme(assets, now_ms);
        }
    }

    pub(crate) fn play_shell_ui_sound_by_id(state: &mut AppState, sound_id: Option<&str>) {
        let Some(sound_id) = sound_id else {
            return;
        };
        let (Some(sfx), Some(catalog)) = (
            &mut state.audio.sfx_player,
            state.process_assets.audio_catalog(),
        ) else {
            return;
        };
        sfx.play_sound(sound_id, catalog.sounds());
    }

    /// `MainMenuDialog0xE2__Run` `0x00531CC0` reads the Keyboard queue while
    /// the bare 0xE2 runs, and only there: every other shell page runs its
    /// own loop. True when that loop owns the key.
    ///
    /// RESIDUAL: VERA drops keys typed during 0xE2's entry slide
    /// (`blocks_shell_input`), and no mouse click reaches the buffer.
    pub(super) fn handle_main_menu_typed_code_key(
        state: &mut AppState,
        event: &winit::event::KeyEvent,
    ) -> bool {
        if state.frontend.screen != GameScreen::MainMenu
            || state.frontend.shell_route != ShellRoute::MainMenu
            || state.frontend.main_menu_shell_error.is_some()
        {
            return false;
        }
        // InputManagerClass__HandleWindowMessage `0x0054F790` queues no
        // key-down repeat (lParam bit 30); 0xE2 skips releases (`0x00531E13`).
        if !event.state.is_pressed() || event.repeat {
            return true;
        }
        let unmodified = event.key_without_modifiers();
        let key = crate::app::input::hotkeys::binding_logical_key(
            &event.logical_key,
            &unmodified,
            event.location,
        );
        // A key with no virtual key here is no letter either.
        let virtual_key =
            crate::app::input::hotkeys::logical_virtual_key(key, event.location).unwrap_or(0);
        let matched = state.frontend.main_menu_shell_state.typed_codes.key_down(
            virtual_key,
            typed_code_video_flags(&mut state.persistence.options_profile),
        );
        if matched.is_none() {
            return true;
        }
        // 0x00531ECB..0x00531EDB: PlayGlobal(Rules->OptionsChanged, centre, 1.0).
        Self::play_rules_sound(state, |general| general.options_changed_sound());
        // RESIDUAL: 0x00531EE4..0x00531EFA paints the empty backdrop with
        // every right-panel tile shuttered (`Shell__DrawEmptyBackdrop
        // 0x0052FEC0`, DL=1) and nothing invalidates 0xE2 afterwards, so
        // retail may briefly show shuttered buttons on a match. VERA redraws
        // the whole menu.
        true
    }

    /// `Dialog0x103__DlgProc`'s global `VocHandle` at `0x00B0BF28`: 0x4EC
    /// starts the looping `ScoreAnimSound`; Continue and WM_DESTROY end it.
    /// `HandleOwner::Global` is the process-global VocHandle kind, unused by
    /// other VERA cues.
    const CAMPAIGN_SCORE_ANIM_HANDLE: u32 = 0;

    pub(crate) fn play_campaign_score_anim_sound(state: &mut AppState, sound_id: Option<&str>) {
        let Some(sound_id) = sound_id else {
            return;
        };
        // 0x4EC runs `VocHandle__StopAndClear` before the new PlayGlobal, so a
        // repeated SHOW completion cannot stack two loops on the handle.
        Self::stop_campaign_score_anim_sound(state);
        let (Some(sfx), Some(catalog)) = (
            &mut state.audio.sfx_player,
            state.process_assets.audio_catalog(),
        ) else {
            return;
        };
        sfx.play_animation_sound_spatial(
            crate::audio::arbiter::HandleOwner::Global(Self::CAMPAIGN_SCORE_ANIM_HANDLE),
            sound_id,
            crate::audio::sfx::SpatialGain::CENTRED_FULL,
            catalog.sounds(),
        );
    }

    pub(crate) fn stop_campaign_score_anim_sound(state: &mut AppState) {
        if let Some(sfx) = state.audio.sfx_player.as_mut() {
            sfx.stop_animation_sound(crate::audio::arbiter::HandleOwner::Global(
                Self::CAMPAIGN_SCORE_ANIM_HANDLE,
            ));
        }
    }

    pub(super) fn handle_single_player_shell_action(
        state: &mut AppState,
        action: crate::ui::single_player_shell::SinglePlayerShellAction,
    ) {
        use crate::ui::single_player_shell::SinglePlayerShellAction as Action;
        match action {
            Action::None => {}
            Action::Skirmish => Self::enter_native_skirmish_from_single_player(state),
            Action::MainMenu => Self::close_single_player_shell(state),
            Action::LoadSavedGame
                if state
                    .frontend
                    .single_player_shell_state
                    .load_saved_game_enabled =>
            {
                Self::open_load_saved_game_page(state)
            }
            Action::LoadSavedGame => {}
            Action::NewCampaign => Self::open_campaign_page(state),
        }
    }

    pub(super) fn handle_main_menu_shell_action(
        state: &mut AppState,
        action: crate::ui::main_menu_shell::MainMenuShellAction,
    ) {
        use crate::ui::main_menu_shell::MainMenuShellAction as Action;
        match action {
            Action::None => {}
            // The original pops a confirm message box here; it does NOT quit on
            // the first Exit click. Quitting happens only on confirm.
            Action::ExitGame => Self::open_exit_confirm(state),
            Action::SinglePlayer => Self::open_single_player_shell(state),
            Action::Options => Self::open_launcher_options_dialog(state),
            Action::MoviesAndCredits => Self::open_movies_credits_page(state),
            Action::Network | Action::WwOnline if state.process_assets.uses_client_catalog() => {
                Self::open_shell_notice(
                    state,
                    "This mod's client catalog is available in Skirmish. Choose No mod under Options, Mods to play online or on a network.".into(),
                );
            }
            // Both lead to a pre-match check, which compares a mod's files.
            Action::Network | Action::WwOnline => {
                state.process_assets.read_mod_files_ahead();
                match action {
                    Action::Network => Self::open_lan_lobby(state),
                    _ => Self::open_wol_welcome_page(state),
                }
            }
        }
    }

    /// Open the Exit-Game confirm message box, resolving its labels from CSF.
    fn open_exit_confirm(state: &mut AppState) {
        // State 6 runs after 0xE2 is destroyed: it paints the empty shell
        // backdrop (`0x0052FEC0`) and asks; Cancel builds a new 0xE2 (state
        // 0x12, `0x0052DE39..0x0052DE42`).
        Self::destroy_current_shell_dialog(state);
        let csf = |key: &str, fallback: &str| state.csf_label(key, fallback);
        let shell_box = crate::ui::main_menu_dialogs::exit_confirm(&csf);
        Self::open_shell_message_box(state, shell_box, Purpose::ExitGame);
    }

    /// ShellMessageBox__Run `0x005D3490` over the empty shell backdrop.
    pub(super) fn open_shell_message_box(
        state: &mut AppState,
        shell_box: crate::ui::shell::modal::ShellBox,
        purpose: Purpose,
    ) {
        // The SHP modal sources PUDLGBGN/MNBTTN from the skirmish chrome atlas; load
        // it on demand so the box renders straight from the main menu.
        let _ = Self::ensure_skirmish_shell_chrome(state);
        // Host the modal as a TRUE LIFO push over the active shell (D-B3):
        // teardown pops back to it with focus restored. (ensure_active would
        // reset_to-clobber the stack — the prior "0x120 over 0xE2" comment
        // described behavior that never happened.)
        let message_box = crate::app::frontend::state::ShellMessageBox { shell_box, purpose };
        state.frontend.shell_message_box = Some(message_box);
        Self::raise_shell_message_box(state);
        state.platform.window.request_redraw();
    }

    /// An OK box that only says `text`, in place of any box already open.
    pub(crate) fn open_shell_notice(state: &mut AppState, text: String) {
        if state.frontend.shell_message_box.is_some() {
            Self::close_shell_message_box_from_controller(state);
        }
        let ok = state.csf_label("TXT_OK", "OK");
        Self::open_shell_message_box(
            state,
            crate::ui::shell::modal::ShellBox::notice(text, ok),
            Purpose::Notice,
        );
    }

    /// Put the open box on top of the controller. A page under the box still
    /// runs, and its network events can reset the controller to itself.
    pub(super) fn raise_shell_message_box(state: &mut AppState) {
        let Some(message_box) = state.frontend.shell_message_box.as_ref() else {
            return;
        };
        let dialog = message_box.shell_box.dialog;
        if state.frontend.shell_controller.top_id() != Some(dialog) {
            state.frontend.shell_controller.push_modal(dialog);
        }
    }

    /// Close retained main-menu child dialogs (options — never on the
    /// controller stack). The exit-confirm modal closes through
    /// close_shell_message_box_from_controller (D-B3).
    /// Controller-routed exit-confirm teardown (D-B3): dismiss the modal UI
    /// state, then LIFO-pop its 0x120 instance so focus returns to the shell
    /// beneath. Mirrors `close_validation_modal_from_controller` — every Esc
    /// and mouse close path converges here.
    pub(super) fn close_shell_message_box_from_controller(state: &mut AppState) {
        state.frontend.shell_message_box = None;
        if matches!(
            state.frontend.shell_controller.top_id(),
            Some(crate::ui::shell::descriptor::DialogResourceId(
                0x00CE | 0x0120 | 0x0121
            ))
        ) {
            state.frontend.shell_controller.pop();
        }
    }

    /// A key on the open box, raised over the page under it: Enter and
    /// Escape answer through its hidden IDOK.
    pub(super) fn route_shell_message_box_key(state: &mut AppState, key: vera20k_gui::Key) {
        let Some(shell_box) = state
            .frontend
            .shell_message_box
            .as_ref()
            .map(|b| b.shell_box.clone())
        else {
            return;
        };
        Self::raise_shell_message_box(state);
        // Add Mod's boxes keep Enter from cancelling the choice or a running
        // import: it answers OK, or nothing while the import prepares.
        #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
        if key == vera20k_gui::Key::Enter
            && let Some(activate) = super::shell_mod_import::enter(state)
        {
            if activate {
                Self::resolve_shell_message_box(state, 0);
            }
            return;
        }
        let parts = shell_layout::shell_box_parts(state, &shell_box);
        let event = vera20k_gui::InputEvent::Key(key);
        if let Some(answer) = Self::shell_box_input(state, &shell_box, &parts, event)
            .and_then(|id| shell_box.answer(id))
        {
            Self::resolve_shell_message_box(state, answer);
        }
    }

    pub(super) fn invalidate_main_menu_movie_if_base_changed(state: &mut AppState) {
        let movie_base = crate::ui::main_menu_shell::movie_base_for_screen_width(
            state.renderer.gpu.config.width,
        );
        if state
            .frontend
            .main_menu_movie_identity
            .is_some_and(|identity| identity.base() != movie_base)
        {
            crate::app::frontend::main_menu_shell_render::clear_ra2ts_movie_session(state);
        }
    }
}

#[cfg(all(test, tests_app))]
mod status_error_tests {
    use super::shell_error_card;
    use crate::app::initialize::{StartupFiles, StartupProblem};
    use std::path::Path;

    #[test]
    fn a_broken_config_or_settings_folder_is_not_called_missing_game_files() {
        let root = std::env::temp_dir().join(format!("vera20k-card-{}", std::process::id()));
        let launch = root.join("launch");
        std::fs::create_dir_all(&launch).unwrap();
        std::fs::create_dir_all(root.join("app")).unwrap();
        let config = launch.join("config.toml");
        std::fs::write(&config, b"[paths]\nra2_dir = 'C:/Gr\xfcn'\n").unwrap();
        let executable = root.join("app/vera20k");
        let error = crate::util::config::GameConfig::load_from(&launch, &executable).unwrap_err();
        let files = StartupFiles {
            config_file: crate::util::config::GameConfig::path_for(&launch, &executable).ok(),
            problem: Some(StartupProblem::of_config_load(&error)),
        };
        let _ = std::fs::remove_dir_all(&root);
        let details = format!("{error:#}");
        let (title, text) = shell_error_card(false, &files, false, None, None, &details);
        assert_eq!(title, "config.toml could not be read");
        assert!(text.contains(config.to_str().unwrap()), "{text}");
        assert!(text.contains("not UTF-8 text"), "{text}");

        let files = StartupFiles {
            config_file: None,
            problem: Some(StartupProblem::Settings(Some("/locked/VERA20k".into()))),
        };
        let (title, text) = shell_error_card(
            true,
            &files,
            false,
            None,
            None,
            "Could not prepare VERA.INI: access was denied (os error 5)",
        );
        assert_eq!(title, "Cannot use the settings folder");
        assert!(text.contains("/locked/VERA20k"), "{text}");
    }

    #[test]
    fn the_game_files_card_gives_this_platforms_advice_and_real_paths() {
        let files = StartupFiles {
            config_file: Some("~/.local/share/VERA20k/config.toml".into()),
            problem: None,
        };
        let log = Path::new("~/.local/share/VERA20k/logs/ra2.log");
        let (title, text) = shell_error_card(false, &files, false, None, Some(log), "missing");
        assert_eq!(title, "Game files not found");
        assert!(!text.contains("vera20k.exe"), "{text}");
        assert!(text.contains("VERA20k/config.toml"), "{text}");
        assert!(text.contains("VERA20k/logs/ra2.log"), "{text}");

        let (_, text) = shell_error_card(true, &files, false, None, Some(log), "missing");
        assert!(text.contains("Copy vera20k.exe"), "{text}");
    }

    #[test]
    fn paths_and_details_in_any_script_reach_the_card_whole() {
        // The card draws VERA's bundled fonts, so a Chinese folder name and a
        // zh-CN host's ERROR_PATH_NOT_FOUND sentence show as they are, with no
        // note about boxes.
        let files = StartupFiles {
            config_file: Some("C:/\u{6e38}\u{620f}/config.toml".into()),
            problem: None,
        };
        let details = "Could not load the game archives: IO error reading asset file: \
\u{7cfb}\u{7edf}\u{627e}\u{4e0d}\u{5230}\u{6307}\u{5b9a}\u{7684}\u{8def}\u{5f84}\u{3002} (os error 3)";
        let (_, text) = shell_error_card(true, &files, false, None, None, details);
        assert!(text.contains("C:/\u{6e38}\u{620f}/config.toml"), "{text}");
        assert!(text.ends_with(&format!("Details: {details}")), "{text}");
        assert!(!text.contains("boxes"), "{text}");
    }
}
