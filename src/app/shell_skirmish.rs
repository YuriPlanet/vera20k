use super::*;
use crate::app::PlatformState;
use crate::app::frontend::shell_layout::{choose_map_layout, skirmish_layout};
use crate::app::frontend::shell_transition::ShellSlideKind;
use crate::app::frontend::state::FrontendState;
use crate::app::input::state::MatchInputState;
use crate::app::match_runtime::session::MatchSession;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::persistence::SaveRepository;
use crate::app::persistence::options_profile::RetailOptionsProfile;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::scenario_catalog::ScenarioCatalog;
use crate::app::shell_main_menu::ClientStep;
use crate::app::shell_route::ShellRoute;
use crate::audio::sfx::SfxPlayer;
use crate::render::bit_font::BitFont;
use crate::ui::shell::controller::DialogController;
use crate::ui::shell::page::PageCue;
use crate::ui::skirmish_shell::SkirmishShellState;
use crate::ui::skirmish_shell::page::Pointer;
use crate::ui::skirmish_shell::{
    ChooseMapModalButton, ChooseMapModalState, ChooseMapPromptKind, ChooseMapSelection,
    SkirmishShellAction, SkirmishShellDialog,
};
use crate::util::config::GameConfig;
use winit::window::Window;

/// Skirmish's validation box (`0xCE`).
const VALIDATION_BOX: crate::ui::shell::descriptor::DialogResourceId =
    crate::ui::shell::descriptor::DialogResourceId(0x00CE);

/// The open Choose Map dialog.
fn chooser(skirmish_shell: &mut SkirmishShellState) -> Option<&mut ChooseMapModalState> {
    skirmish_shell.choose_map_modal.as_mut()
}

impl App {
    /// VERA's Team Starts in the game room: the start cells
    /// of shell map `map` by start index. A map gives its start waypoints;
    /// the random map gives the starts of the last Generate
    /// (`0x0058B820`'s header), none before one.
    pub(crate) fn map_start_cells(frontend: &FrontendState, map: usize) -> Vec<(i32, i32)> {
        let Some(entry) = frontend.scenario_catalog.shell_maps().get(map) else {
            return Vec::new();
        };
        if crate::app::frontend::skirmish_shell_render::is_random_map_sentinel_entry(entry) {
            let header = frontend.random_map_retention.generated_header();
            let count = usize::try_from(header.count).unwrap_or(0);
            return header
                .cells
                .iter()
                .take(count)
                .map(|&(x, y)| (i32::from(x), i32::from(y)))
                .collect();
        }
        // The Start column names a start by waypoint index, so only the
        // leading unbroken run 0, 1, 2... can be given out by position.
        entry
            .multiplayer_start_waypoints
            .iter()
            .enumerate()
            .take_while(|(position, waypoint)| waypoint.index as usize == *position)
            .map(|(_, waypoint)| (i32::from(waypoint.rx), i32::from(waypoint.ry)))
            .collect()
    }

    /// VERA's Team Starts: how far a ground unit drives between shell map
    /// `map`'s `starts`, by start index, or `None` where the room cannot read
    /// the map's terrain (the random map, or no resident assets or Rules) and
    /// Team Starts measures straight lines. Each call reads the map file and
    /// its theater, which makes that theater's archives the active ones until
    /// a Generate or a match load picks its own.
    pub(crate) fn map_drive_distances(
        game_config: Option<&GameConfig>,
        scenario_catalog: &ScenarioCatalog,
        process_assets: &mut ProcessAssets,
        map: usize,
        starts: &[(i32, i32)],
    ) -> Option<Vec<Vec<Option<u32>>>> {
        let entry = scenario_catalog.shell_maps().get(map)?;
        if crate::app::frontend::skirmish_shell_render::is_random_map_sentinel_entry(entry) {
            return None;
        }
        let file_name = entry.file_name.clone();
        let cells = starts
            .iter()
            .map(|&(x, y)| Some((u16::try_from(x).ok()?, u16::try_from(y).ok()?)))
            .collect::<Option<Vec<_>>>()?;
        let ra2_dir = game_config?.paths.ra2_dir.clone();
        let (assets, rules, _) = process_assets.manager_and_rules_with_tile_cache();
        let (assets, rules) = (assets?, rules?);
        let map = match crate::map::source::load_map_by_name_or_path_with_assets(
            &ra2_dir, &file_name, assets,
        ) {
            Ok(loaded) => loaded.map,
            Err(error) => {
                log::warn!("Team Starts measures straight lines: map {file_name}: {error}");
                return None;
            }
        };
        let Ok(Some(theater)) = crate::map::theater::load_scenario_theater(
            assets,
            &map.header.theater,
            &map.ini,
            false,
        ) else {
            log::warn!(
                "Team Starts measures straight lines: theater {} unavailable",
                map.header.theater
            );
            return None;
        };
        Some(
            crate::sim::pathfinding::drive_distance::map_drive_distances(
                &map,
                &theater,
                assets,
                rules.selected_rules_root(),
                rules.fixed_art(),
                &cells,
            ),
        )
    }

    /// The Choose Map search line takes keys only while its page is active.
    /// Its visible rectangle also anchors the platform's IME candidate UI.
    pub(super) fn choose_map_search_ime_rect(
        platform: &PlatformState,
        renderer: &RendererState,
        frontend: &FrontendState,
        match_state: &MatchState,
        persistence: &PersistenceState,
    ) -> Option<crate::ui::shell::geom::RectPx> {
        use crate::app::input::precedence::{InputLayer, ScreenPage};
        if !matches!(
            crate::app::input::precedence::input_layer(
                platform,
                renderer,
                frontend,
                match_state,
                persistence
            ),
            InputLayer::Screen(ScreenPage::NetworkSetup | ScreenPage::Skirmish)
        ) {
            return None;
        }
        let modal = (frontend.skirmish_shell_state.choose_map_modal.as_ref())
            .filter(|modal| modal.message_box.is_none() && !modal.is_hidden())?;
        let layout = choose_map_layout(renderer, &frontend.screen);
        Some(
            crate::app::frontend::skirmish_shell_render::choose_map_search_ime_area(
                &platform.ime,
                &renderer.bit_font,
                &layout,
                &modal.search,
            ),
        )
    }

    /// Android: one Backspace in the chooser's search line, through the same
    /// GUI input the desktop key reaches.
    #[cfg(target_os = "android")]
    pub(crate) fn choose_map_search_backspace(state: &mut AppState) -> bool {
        Self::choose_map_search_input(
            &state.platform,
            &state.renderer,
            &mut state.frontend,
            &state.match_state,
            &state.persistence,
            vera20k_gui::InputEvent::Key(vera20k_gui::Key::Backspace),
        )
    }

    /// Apply committed input-method text to Choose Map's search line, as
    /// the ordinary key path does; its control characters are dropped.
    pub(super) fn choose_map_search_text(
        platform: &PlatformState,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        match_state: &MatchState,
        persistence: &PersistenceState,
        text: &str,
    ) -> bool {
        let text = text.chars().filter(|ch| !ch.is_control()).collect();
        Self::choose_map_search_input(
            platform,
            renderer,
            frontend,
            match_state,
            persistence,
            vera20k_gui::InputEvent::Text(text),
        )
    }

    /// Typed text or Backspace for the search line of a Choose Map that
    /// takes keys (see [`Self::choose_map_search_ime_rect`]), through its
    /// edit; false when none does.
    fn choose_map_search_input(
        platform: &PlatformState,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        match_state: &MatchState,
        persistence: &PersistenceState,
        event: vera20k_gui::InputEvent,
    ) -> bool {
        if Self::choose_map_search_ime_rect(platform, renderer, frontend, match_state, persistence)
            .is_none()
        {
            return false;
        }
        let layout = choose_map_layout(renderer, &frontend.screen);
        let Some(modal) = frontend.skirmish_shell_state.choose_map_modal.as_mut() else {
            return false;
        };
        crate::ui::skirmish_shell::choose_map_page::search_input(
            &mut frontend.shell_controller,
            modal,
            &layout,
            (
                &frontend.skirmish_modes,
                frontend.scenario_catalog.records(),
            ),
            event,
        );
        platform.window.request_redraw();
        true
    }

    /// The page selected another map: the random-map retention follows it
    /// and the preview is drawn again.
    pub(super) fn note_selected_shell_map(frontend: &mut FrontendState, file_name: &str) {
        frontend.random_map_retention.select_map(file_name);
        frontend.skirmish_preview_texture = None;
    }

    /// Skirmish Back (`0x5C0`): the proc packs the session like Start does
    /// (`0x006ACEE0`), then the runner tears the dialog down: the teardown
    /// slide back to the Single Player page `0x100`, which is the returned
    /// step.
    pub(super) fn handle_skirmish_back(frontend: &mut FrontendState) -> ClientStep {
        Self::note_skirmish_scenario(frontend);
        match crate::ui::skirmish_shell::pack_launch_session_without_start_validation(
            &frontend.skirmish_shell_state,
            frontend.scenario_catalog.shell_maps(),
            &frontend.skirmish_modes,
        ) {
            Ok(raw_session) => {
                if let Err(err) = frontend.multiplayer_shell_runtime.close_shell_transaction(
                    &frontend.skirmish_shell_state,
                    frontend.scenario_catalog.shell_maps(),
                    &frontend.skirmish_modes,
                    &raw_session,
                ) {
                    // Invalid Cooperative content has no parity-safe
                    // retry cap. Keep Back usable, surface the malformed
                    // data, and retain every draw consumed before error.
                    log::error!("Could not complete Cooperative Back randomization: {err}");
                }
            }
            Err(err) => {
                log::warn!("Could not pack raw Skirmish Back session: {err:?}");
            }
        }
        ClientStep::Leave(
            ShellSlideKind::Skirmish,
            Box::new(|state: &mut AppState| {
                App::commit_skirmish_back(&mut state.frontend, &state.persistence.repository)
            }),
        )
    }

    /// Skirmish Start after the teardown slide: `0x006AE2C0` destroys the
    /// dialog, writes the session fields (`0x006990A0`) and returns true, and
    /// Main_Game launches the scenario.
    pub(super) fn commit_skirmish_start(
        state: &mut AppState,
        session: crate::skirmish_launch::SkirmishLaunchSession,
    ) {
        Self::teardown_skirmish_shell(&mut state.frontend);
        state.frontend.multiplayer_shell_runtime.persist_snapshot();
        if let Err(error) = Self::start_skirmish_session(
            state,
            session,
            &mut crate::app::match_bootstrap::OrdinaryMatchSeedClock,
        ) {
            log::error!("Cannot start skirmish: {error:#}");
        }
    }

    /// Skirmish Back after the teardown slide: the session fields are written
    /// (`0x006990A0`), then state 1 builds the Single Player page.
    pub(super) fn commit_skirmish_back(frontend: &mut FrontendState, repository: &SaveRepository) {
        frontend.multiplayer_shell_runtime.persist_snapshot();
        Self::teardown_skirmish_shell(frontend);
        Self::open_single_player_shell(frontend, repository);
    }

    /// `0x102` closes: its boxes and the name edit's focus go with it.
    pub(super) fn teardown_skirmish_shell(frontend: &mut FrontendState) {
        frontend.shell_route = ShellRoute::MainMenu;
        frontend.shell_first_paint_slide = None;
        frontend.skirmish_shell_state.choose_map_modal = None;
        frontend.skirmish_shell_state.validation_modal = None;
        frontend.skirmish_shell_state.game_options = None;
        Self::blur_player_name(&mut frontend.shell_controller);
        frontend.skirmish_preview_texture = None;
    }

    pub(super) fn start_skirmish_session(
        state: &mut AppState,
        session: crate::skirmish_launch::SkirmishLaunchSession,
        clock: &mut impl crate::app::match_bootstrap::MatchSeedClock,
    ) -> anyhow::Result<()> {
        let accepted_random_map = state
            .frontend
            .random_map_retention
            .take_acceptance_for_loading(session.selected_map_file.as_deref());
        // A rematch of the same random map: the first launch consumed the
        // accepted setup, so the shell's retained staging supplies the starts.
        let rematch_staging = accepted_random_map
            .is_none()
            .then(|| {
                crate::app::shell_random_map::AcceptedRmgStartStaging::from_retained_staging(
                    &state.frontend.multiplayer_shell_runtime.start_staging(),
                    session.selected_map_file.as_deref(),
                )
            })
            .flatten();
        let request = match crate::app::match_bootstrap::classify_startup_session(&session) {
            crate::app::match_bootstrap::StartupSessionClassification::AcceptedExplicitFixedBattle(
                accepted,
            ) => {
                let correlation = crate::app::match_bootstrap::allocate_match_correlation(
                    &mut state.frontend.next_match_correlation,
                )?;
                let startup = crate::app::match_bootstrap::prepare_match_startup(
                    correlation,
                    *accepted,
                    clock,
                );
                crate::app::loading::pump::LoadingRequest::accepted_skirmish(startup)
            }
            crate::app::match_bootstrap::StartupSessionClassification::UnverifiedLegacy(reason) => {
                log::warn!("Skirmish startup uses unverified compatibility path: {reason:?}");
                let seed = crate::app::match_bootstrap::read_match_seed(clock);
                crate::app::loading::pump::LoadingRequest::unverified_legacy_skirmish(session, seed)
            }
        }
        .with_accepted_random_map(accepted_random_map)
        .with_start_staging(rematch_staging);
        // GameMode stays 5 through the game, so the shell resumes on a new
        // `0x102` afterwards (`App::resume_shell_after_match`).
        state.frontend.shell_route = ShellRoute::Skirmish;
        state.frontend.shell_first_paint_slide = None;
        state.frontend.skirmish_preview_texture = None;
        crate::app::loading::pump::begin_loading(state, request);
        state.match_state.input.zoom_level = 1.0;
        state.match_state.input.zoom_target = 1.0;
        Ok(())
    }

    pub(crate) fn ensure_skirmish_shell_chrome(
        renderer: &RendererState,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        options_profile: &RetailOptionsProfile,
    ) -> bool {
        let yr_original_style = options_profile.plays_yr_original_style();
        if frontend
            .skirmish_shell_chrome
            .as_ref()
            .is_some_and(|atlas| atlas.yr_original_style == yr_original_style)
        {
            return true;
        }

        // Drop a stale derived atlas before trying to build the requested
        // style. A failed rebuild must not keep painting the old profile.
        frontend.skirmish_shell_chrome = None;
        let Some(assets) = process_assets.manager() else {
            log::warn!(
                "Could not prepare Skirmish shell chrome: process asset manager is unavailable"
            );
            return false;
        };

        // Ares' country flags join the atlas when Ares.dll is loaded (the
        // game is read the Ares way).
        let ares = process_assets.reads_ares_way();
        let extra_flags = if ares {
            process_assets
                .native_rules()
                .map(|rules| rules.country_flag_files())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let dialog_background = frontend.shell_dialog_background.clone();
        let started = crate::util::time::Instant::now();
        frontend.skirmish_shell_chrome =
            crate::render::skirmish_shell_chrome::build_skirmish_shell_chrome_atlas(
                &renderer.gpu,
                &renderer.batch_renderer,
                assets,
                yr_original_style,
                &extra_flags,
                dialog_background
                    .as_ref()
                    .map(|(image, palette)| (image.as_str(), palette.as_str())),
            );
        let ready = frontend.skirmish_shell_chrome.is_some();
        if ready {
            // The pack above logged the atlas size; this is the whole build,
            // the decoding of its SHPs included.
            log::info!(
                "Skirmish shell chrome built in {:.0} ms",
                started.elapsed().as_secs_f64() * 1e3
            );
        }
        if !ready {
            log::warn!(
                "Could not prepare Skirmish shell chrome from the registered retail archives"
            );
        }
        ready
    }

    /// A `0x102` button's action; Start Game, Back and Choose Map return
    /// their leave as the step.
    #[must_use]
    pub(super) fn handle_skirmish_shell_action(
        window: &Window,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        options_profile: &RetailOptionsProfile,
        action: SkirmishShellAction,
    ) -> Option<ClientStep> {
        match action {
            SkirmishShellAction::StartGame => {
                Self::start_game_from_shell(window, frontend, process_assets, options_profile)
            }
            SkirmishShellAction::BackOrExit => Some(Self::handle_skirmish_back(frontend)),
            // `0x102` slides out (not torn down) before `0x6B` runs.
            SkirmishShellAction::ChooseMap => Some(ClientStep::Leave(
                ShellSlideKind::Skirmish,
                Box::new(|state: &mut AppState| {
                    App::open_choose_map_from_skirmish(
                        &state.platform,
                        &state.renderer,
                        &mut state.frontend,
                        &state.match_state,
                        &mut state.process_assets,
                    )
                }),
            )),
            SkirmishShellAction::GameOptions => {
                Self::open_skirmish_game_options(
                    window,
                    &mut frontend.skirmish_shell_state,
                    options_profile,
                );
                None
            }
            SkirmishShellAction::Setups => {
                Self::open_skirmish_setups(renderer, frontend, process_assets);
                None
            }
            SkirmishShellAction::None => None,
        }
    }

    /// VERA's Game Options over `0x102`, on the rules the profile keeps.
    fn open_skirmish_game_options(
        window: &Window,
        skirmish_shell: &mut SkirmishShellState,
        options_profile: &RetailOptionsProfile,
    ) {
        use crate::ui::skirmish_shell::game_options::{GameOptionsBox, MatchRules};
        let profile = options_profile;
        let rules = MatchRules {
            superweapon_limit: profile.superweapon_limit,
            terrain_deformation: profile.terrain_deformation,
            skirmish_cheats: profile.skirmish_cheats,
            session: skirmish_shell.session,
            short_game: skirmish_shell.short_game,
        };
        skirmish_shell.game_options = Some(GameOptionsBox::new(rules));
        window.request_redraw();
    }

    /// OK, Enter or Esc on the Game Options box: it closes, and the next
    /// game's rules and VERA.INI take what it shows.
    pub(super) fn close_skirmish_game_options(
        platform: &PlatformState,
        frontend: &mut FrontendState,
        process_assets: &mut ProcessAssets,
        options_profile: &mut RetailOptionsProfile,
    ) {
        let shell = &mut frontend.skirmish_shell_state;
        let Some(open) = shell.game_options.take() else {
            return;
        };
        let controller = &mut frontend.shell_controller;
        if controller.top_id() == Some(crate::ui::skirmish_shell::game_options::DIALOG) {
            controller.pop();
        }
        let rules = open.rules;
        // The page keeps the session options with its own, and the Short
        // Game unticking Bases may have unticked.
        shell.session = rules.session;
        shell.short_game = rules.short_game;
        let profile = &mut *options_profile;
        let shared_rules_changed = (profile.superweapon_limit, profile.terrain_deformation)
            != (rules.superweapon_limit, rules.terrain_deformation);
        if shared_rules_changed {
            profile.superweapon_limit = rules.superweapon_limit;
            profile.terrain_deformation = rules.terrain_deformation;
            // The next offline game reads them, and a room's host starts
            // its room with them.
            process_assets.set_superweapon_limit(i32::from(rules.superweapon_limit));
            process_assets.set_terrain_deformation(rules.terrain_deformation);
        }
        if shared_rules_changed || profile.skirmish_cheats != rules.skirmish_cheats {
            profile.skirmish_cheats = rules.skirmish_cheats;
            crate::app::persistence::options::persist_options_profile(platform, options_profile);
        }
        platform.window.request_redraw();
    }

    /// The open Game Options box takes every press, release and move over
    /// the page; returns whether it is open.
    /// A fresh Enter or Escape over the Game Options box: its OK takes both
    /// and the box closes.
    pub(super) fn skirmish_game_options_key(
        platform: &PlatformState,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        process_assets: &mut ProcessAssets,
        options_profile: &mut RetailOptionsProfile,
        key: vera20k_gui::Key,
    ) {
        use crate::ui::skirmish_shell::game_options;
        let page = skirmish_layout(renderer, frontend);
        let close = game_options::key(
            &mut frontend.shell_controller,
            &mut frontend.skirmish_shell_state,
            &game_options::GameOptionsLayout::over(&page),
            key,
        );
        if close {
            Self::close_skirmish_game_options(platform, frontend, process_assets, options_profile);
        }
    }

    fn route_game_options_mouse(
        platform: &PlatformState,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        match_state: &MatchState,
        process_assets: &mut ProcessAssets,
        sfx_player: &mut Option<SfxPlayer>,
        options_profile: &mut RetailOptionsProfile,
        at: Pointer,
    ) -> bool {
        use crate::ui::skirmish_shell::game_options;
        if frontend.skirmish_shell_state.game_options.is_none() {
            return false;
        }
        let page = skirmish_layout(renderer, frontend);
        let (x, y) = match_state.input.cursor_px();
        let (cues, close) = game_options::pointer(
            &mut frontend.shell_controller,
            &mut frontend.skirmish_shell_state,
            &game_options::GameOptionsLayout::over(&page),
            at,
            x,
            y,
        );
        Self::play_cues(frontend, match_state, process_assets, sfx_player, cues);
        if close {
            Self::close_skirmish_game_options(platform, frontend, process_assets, options_profile);
        }
        platform.window.request_redraw();
        if at != Pointer::Down {
            Self::update_skirmish_shell_status_help(
                &platform.window,
                frontend,
                process_assets,
                x,
                y,
            );
        }
        true
    }

    /// Start Game on `0x102` (`0x006ACEE0` case `0x617`): validate and pack
    /// the session, then slide the dialog out before the launch. That slide
    /// is the returned step.
    #[must_use]
    pub(crate) fn start_game_from_shell(
        window: &Window,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        options_profile: &RetailOptionsProfile,
    ) -> Option<ClientStep> {
        Self::note_skirmish_scenario(frontend);
        match crate::ui::skirmish_shell::launch_session(
            &frontend.skirmish_shell_state,
            frontend.scenario_catalog.shell_maps(),
            &frontend.skirmish_modes,
            crate::ui::skirmish_shell::start_skips_side_gates(
                options_profile.solo_skirmish,
                frontend.frontend_rules.as_ref(),
            ),
        ) {
            Ok(raw_session) => {
                match frontend.multiplayer_shell_runtime.close_shell_transaction(
                    &frontend.skirmish_shell_state,
                    frontend.scenario_catalog.shell_maps(),
                    &frontend.skirmish_modes,
                    &raw_session,
                ) {
                    // 0x006ACEE0 packs the session and writes result 0x617;
                    // the runner (0x006AE2C0) then slides the dialog out
                    // before the launch.
                    Ok(session) => Some(ClientStep::Leave(
                        ShellSlideKind::Skirmish,
                        Box::new(move |state: &mut AppState| {
                            Self::commit_skirmish_start(state, session)
                        }),
                    )),
                    Err(err) => {
                        log::error!("Could not resolve Cooperative shell assignments: {err}");
                        window.request_redraw();
                        None
                    }
                }
            }
            Err(err) => {
                Self::show_skirmish_validation_box(window, frontend, process_assets, &err);
                None
            }
        }
    }

    /// Start's validation box `0xCE` for `err`, which takes the name edit's
    /// focus.
    fn show_skirmish_validation_box(
        window: &Window,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        err: &crate::skirmish_launch::LaunchValidationError,
    ) {
        use crate::skirmish_launch::LaunchValidationError as Error;
        let message = match err {
            Error::MapCapacityExceeded { capacity, .. } => process_assets
                .csf_label(
                    "TXT_SCENARIO_TOO_SMALL",
                    "This map has a %d player max. The max includes human and computer players.",
                )
                .replace("%d", &capacity.to_string()),
            Error::NoEnabledOpponent => process_assets.csf_label(
                "TXT_NEED_AT_LEAST_TWO_PLAYERS",
                "You need at least two players to start the game!",
            ),
            Error::SameExplicitTeam { .. } => process_assets.csf_label(
                "TXT_CANNOT_ALLY",
                "Must have more than one team to start a game!",
            ),
            // VERA: map archives are optional, so the list can be empty.
            Error::NoSelectedMap => crate::i18n::tr("VERA:NoMapsFound"),
            _ => return log::warn!("Could not start skirmish shell session: {err:?}"),
        };
        let ok = process_assets.csf_label("TXT_OK", "OK");
        let shell = &mut frontend.skirmish_shell_state;
        shell.validation_modal = Some(crate::ui::shell::modal::ShellBox::notice(message, ok));
        Self::blur_player_name(&mut frontend.shell_controller);
        frontend.shell_controller.ensure_active(VALIDATION_BOX);
        window.request_redraw();
    }

    /// Skirmish Choose Map (`0x5AA`) after `0x102` slid out: `0x6B` runs on
    /// the page's mode and map.
    pub(super) fn open_choose_map_from_skirmish(
        platform: &PlatformState,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        match_state: &MatchState,
        process_assets: &mut ProcessAssets,
    ) {
        let mode_id = frontend.skirmish_shell_state.selected_mode.id;
        let record_index = Self::current_choose_map_record_index(frontend);
        Self::open_choose_map_modal(
            platform,
            renderer,
            frontend,
            match_state,
            process_assets,
            mode_id,
            record_index,
        );
    }

    /// Opens the shared chooser on the caller's current mode and map. Each
    /// dialog supplies its own selection; neither writes the other's state.
    pub(super) fn open_choose_map_modal(
        platform: &PlatformState,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        match_state: &MatchState,
        process_assets: &mut ProcessAssets,
        selected_mode_id: i32,
        current_record_index: Option<usize>,
    ) {
        (frontend.skirmish_shell_state.statics).hover("", crate::util::time::Instant::now());
        // A fresh `0x6B`: its widgets build on the first sync.
        crate::ui::skirmish_shell::choose_map_page::close(&mut frontend.shell_controller);
        // VeraX: Skirmish rescans the game directory each time the chooser
        // opens, so a map copied after launch appears without a restart. A
        // network lobby keeps its launch snapshot, because the map list is
        // local and peers must still compare the same catalog.
        #[cfg(feature = "verax")]
        let current_record_index = if frontend.shell_route == ShellRoute::NetworkSetup {
            current_record_index
        } else {
            Self::reload_skirmish_catalog(platform, frontend, process_assets, current_record_index)
        };
        let layout = choose_map_layout(renderer, &frontend.screen);
        let session = &match_state.session;
        let context = if frontend.shell_route == ShellRoute::NetworkSetup {
            let room = session.network_room();
            crate::ui::skirmish_shell::ChooseMapContext::Network {
                observer: (session.network_setup())
                    .is_some_and(|launch| launch.local_player().is_observer()),
                tournament_room: room.tournament_or_clan(),
                clan_room: room.clan,
            }
        } else {
            crate::ui::skirmish_shell::ChooseMapContext::Skirmish
        };
        frontend.skirmish_shell_state.choose_map_modal = Some(ChooseMapModalState::open(
            context,
            selected_mode_id,
            current_record_index,
            &frontend.skirmish_modes,
            frontend.scenario_catalog.records(),
            &layout,
        ));
        // VERA: the starred maps list under Favorites, and online rooms list
        // the online map library under its own row.
        let favorites = crate::app::map_library::read_favorites(platform, frontend);
        let online = crate::app::map_library::listed_in_choose_map(frontend, match_state);
        if let Some(modal) = frontend.skirmish_shell_state.choose_map_modal.as_mut() {
            if crate::app::worker::THREADS {
                modal.add_random_maps_row(&frontend.skirmish_modes);
            }
            modal.add_favorites_row(favorites);
            if online {
                modal.add_online_maps_row();
                modal.set_online_categories(frontend.map_library.folders());
            }
        }
        Self::list_cooperative_campaigns(
            renderer,
            frontend,
            &match_state.session,
            process_assets,
            false,
        );
    }

    /// List files only when the folder is visible, including when Save or
    /// Delete changes it in the generator above the chooser.
    pub(super) fn refresh_saved_random_maps(
        game_config: Option<&GameConfig>,
        renderer: &RendererState,
        frontend: &mut FrontendState,
    ) {
        let dir = Self::random_map_dir(game_config);
        let layout =
            crate::app::frontend::shell_layout::choose_map_layout(renderer, &frontend.screen);
        if let Some(modal) = frontend
            .skirmish_shell_state
            .choose_map_modal
            .as_mut()
            .filter(|modal| modal.random_maps_selected())
        {
            let seeds = dir
                .map(|dir| crate::map::rmg::saved_seeds::list_saved_seeds(&dir))
                .unwrap_or_default();
            modal.set_saved_seeds(seeds, &layout);
        }
    }

    /// VeraX map hot reload: re-run the retail Choose Map scan over the game
    /// directory and replace the catalog.
    ///
    /// The scan registers loose YRO archives; `register_loose_yro_archive`
    /// skips names the manager already knows, so rescanning the same
    /// process-sticky manager neither duplicates archives nor loses its CRC
    /// cache. An empty or failed scan keeps the launch list.
    #[cfg(feature = "verax")]
    fn reload_skirmish_catalog(
        platform: &PlatformState,
        frontend: &mut FrontendState,
        process_assets: &mut ProcessAssets,
        current_record_index: Option<usize>,
    ) -> Option<usize> {
        let Some(config) = platform.game_config.as_ref() else {
            return current_record_index;
        };
        let ra2_dir = config.paths.ra2_dir.clone();
        let selected_file = current_record_index
            .and_then(|index| frontend.scenario_catalog.records().get(index))
            .map(|record| record.file_name.clone());
        let (Some(assets), csf) = process_assets.scenario_rescan_sources() else {
            return current_record_index;
        };
        let Some(records) = Self::scan_skirmish_records(&ra2_dir, assets, csf) else {
            return current_record_index;
        };
        let remapped =
            Self::remap_record_index(selected_file.as_deref(), current_record_index, &records);
        frontend.scenario_catalog =
            crate::app::scenario_catalog::ScenarioCatalog::from_records(records);
        crate::app::map_library::append_online_records(platform, frontend, process_assets);
        remapped
    }

    /// One Choose Map scan through the process manager. `None` leaves the
    /// caller's catalog untouched: no Skirmish scenarios or an unreadable
    /// directory is a failed reload, not an empty map list.
    #[cfg(feature = "verax")]
    fn scan_skirmish_records(
        ra2_dir: &std::path::Path,
        assets: &mut crate::assets::asset_manager::AssetManager,
        csf: Option<&crate::assets::csf_file::CsfFile>,
    ) -> Option<Vec<crate::map::skirmish_scenarios::SkirmishScenarioRecord>> {
        match crate::map::scenario_sources::list_skirmish_scenario_records_with_assets(
            ra2_dir, assets, csf,
        ) {
            Ok(records) if !records.is_empty() => Some(records),
            Ok(_) => {
                log::warn!("VeraX map reload found no Skirmish scenarios; keeping the launch list");
                None
            }
            Err(error) => {
                log::warn!("VeraX map reload could not scan the game directory: {error:#}");
                None
            }
        }
    }

    /// Keep the selected map across a rescan by file name. A removed map falls
    /// back to the nearest surviving index; a network list or a still-valid
    /// index is unchanged.
    #[cfg(feature = "verax")]
    fn remap_record_index(
        selected_file: Option<&str>,
        previous: Option<usize>,
        records: &[crate::map::skirmish_scenarios::SkirmishScenarioRecord],
    ) -> Option<usize> {
        if records.is_empty() {
            return None;
        }
        selected_file
            .and_then(|name| {
                records
                    .iter()
                    .position(|record| record.file_name.eq_ignore_ascii_case(name))
            })
            .or_else(|| previous.map(|index| index.min(records.len() - 1)))
    }

    /// Cooperative fills Choose Map's list itself (`+0x4C`), on the network
    /// only: Skirmish never lists the mode.
    fn list_cooperative_campaigns(
        renderer: &RendererState,
        frontend: &mut FrontendState,
        session: &MatchSession,
        process_assets: &ProcessAssets,
        clicked: bool,
    ) {
        let Some(modal) = chooser(&mut frontend.skirmish_shell_state) else {
            return;
        };
        let (mode_id, records) = (
            modal.selected_mode.id,
            modal.filtered_record_indices.clone(),
        );
        let Some(rows) = crate::app::shell_network_setup::cooperative_chooser_rows(
            frontend,
            session,
            process_assets,
            mode_id,
            &records,
            clicked,
        ) else {
            return;
        };
        let layout = choose_map_layout(renderer, &frontend.screen);
        if let Some(modal) = frontend.skirmish_shell_state.choose_map_modal.as_mut() {
            modal.replace_rows(rows, frontend.scenario_catalog.records(), &layout, clicked);
        }
    }

    /// `[0xA8B254]` as the Skirmish dialog leaves it: its OnSetup and each
    /// map change write the selection's record (`0x006AEB55`, `0x006AD961`,
    /// `0x006ADB4B`).
    fn note_skirmish_scenario(frontend: &mut FrontendState) {
        let index = Self::current_choose_map_record_index(frontend).unwrap_or(0);
        frontend.multiplayer_shell_runtime.set_scenario_index(index);
    }

    pub(super) fn current_choose_map_record_index(frontend: &FrontendState) -> Option<usize> {
        let catalog = &frontend.scenario_catalog;
        let map = catalog
            .shell_maps()
            .get(frontend.skirmish_shell_state.selected_map_idx)?;
        (catalog.records().iter())
            .position(|record| record.file_name.eq_ignore_ascii_case(&map.file_name))
    }

    pub(super) fn close_choose_map_modal(frontend: &mut FrontendState) {
        crate::ui::skirmish_shell::choose_map_page::close(&mut frontend.shell_controller);
        frontend.skirmish_shell_state.choose_map_modal = None;
    }

    pub(super) fn commit_choose_map_selection(
        game_config: Option<&GameConfig>,
        frontend: &mut FrontendState,
        match_state: &mut MatchState,
        process_assets: &ProcessAssets,
        selection: ChooseMapSelection,
    ) -> bool {
        let catalog = &frontend.scenario_catalog;
        let Some(record) = selection
            .record_index
            .and_then(|i| catalog.records().get(i))
        else {
            return false;
        };
        let Some(map_idx) = (catalog.shell_maps().iter())
            .position(|map| map.file_name.eq_ignore_ascii_case(&record.file_name))
        else {
            let file = &record.file_name;
            log::warn!("Choose Map selected {file}, but no loadable map entry exists yet");
            return false;
        };
        let file_name = catalog.shell_maps()[map_idx].file_name.clone();
        if frontend.shell_route == ShellRoute::NetworkSetup {
            // `+0x18` reads the list's selected row (`0x005E66D0`).
            let row = chooser(&mut frontend.skirmish_shell_state)
                .and_then(|modal| modal.highlighted_filtered_index);
            let mode = selection.mode.id;
            return crate::app::shell_network_setup::commit_map_selection(
                game_config,
                frontend,
                match_state,
                process_assets,
                mode,
                map_idx,
                row,
            )
            .inspect_err(|error| log::warn!("Could not accept LAN map selection: {error:#}"))
            .is_ok();
        }
        let shell = &mut frontend.skirmish_shell_state;
        shell.selected_mode = selection.mode;
        crate::ui::skirmish_shell::repair_teams_for_selected_mode(shell, &frontend.skirmish_modes);
        let maps = frontend.scenario_catalog.shell_maps();
        crate::ui::skirmish_shell::accept_selected_map(shell, maps, map_idx);
        Self::note_selected_shell_map(frontend, &file_name);
        // `0x102` shows its statics again when its entry slide ends; a changed
        // game type or map name restarts that reveal (`0x006ADA99`, `0x4B2`).
        true
    }

    /// A press on Choose Map: whether the chooser is open, and the step of a
    /// button the press activates.
    #[must_use]
    pub(super) fn handle_choose_map_modal_mouse_down(
        platform: &PlatformState,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        match_state: &MatchState,
        process_assets: &ProcessAssets,
        sfx_player: &mut Option<SfxPlayer>,
    ) -> (bool, Option<ClientStep>) {
        if chooser(&mut frontend.skirmish_shell_state).is_none() {
            return (false, None);
        }
        use crate::app::frontend::preview_zoom;
        let over = preview_zoom::over_box(
            &frontend.shell_controller,
            &match_state.input,
            crate::ui::skirmish_shell::choose_map_page::DIALOG,
        );
        if preview_zoom::press_on_box(frontend, &match_state.input, over) {
            return (true, None);
        }
        let step = Self::choose_map_pointer(
            platform,
            renderer,
            frontend,
            match_state,
            process_assets,
            sfx_player,
            Pointer::Down,
        );
        (true, step)
    }

    /// The pointer on the chooser's widgets: their cues play, a game-type
    /// click lists the mode's own rows and a button acts; that button's
    /// step is returned.
    #[must_use]
    fn choose_map_pointer(
        platform: &PlatformState,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        match_state: &MatchState,
        process_assets: &ProcessAssets,
        sfx_player: &mut Option<SfxPlayer>,
        at: Pointer,
    ) -> Option<ClientStep> {
        let layout = choose_map_layout(renderer, &frontend.screen);
        let (x, y) = match_state.input.cursor_px();
        let modal = frontend.skirmish_shell_state.choose_map_modal.as_mut()?;
        let input = crate::ui::skirmish_shell::choose_map_page::pointer(
            &mut frontend.shell_controller,
            modal,
            &layout,
            (
                &frontend.skirmish_modes,
                frontend.scenario_catalog.records(),
            ),
            &mut frontend.choose_map_last_mode_row,
            at,
            x,
            y,
        );
        Self::play_cues(
            frontend,
            match_state,
            process_assets,
            sfx_player,
            input.cues,
        );
        if input.rebuilt {
            Self::refresh_saved_random_maps(platform.game_config.as_ref(), renderer, frontend);
            Self::list_cooperative_campaigns(
                renderer,
                frontend,
                &match_state.session,
                process_assets,
                true,
            );
        }
        platform.window.request_redraw();
        let button = input.activated?;
        Self::choose_map_button(platform, renderer, frontend, &match_state.session, button)
    }

    /// Scrollbar arrow auto-repeat on the chooser's lists; returns the next
    /// wake deadline.
    pub(super) fn poll_choose_map_scroll(
        window: &Window,
        renderer: &RendererState,
        frontend: &mut FrontendState,
    ) -> Option<Instant> {
        let layout = choose_map_layout(renderer, &frontend.screen);
        let modal = frontend.skirmish_shell_state.choose_map_modal.as_mut()?;
        let (moved, next) = crate::ui::skirmish_shell::choose_map_page::poll(
            &mut frontend.shell_controller,
            modal,
            &layout,
        );
        if moved {
            window.request_redraw();
        }
        next
    }

    /// Arrow repeat on VERA's scrollbar beside the Skirmish computer rows;
    /// returns the next wake deadline.
    pub(super) fn poll_skirmish_opponent_scroll(
        platform: &PlatformState,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        match_state: &MatchState,
        persistence: &PersistenceState,
    ) -> Option<Instant> {
        use crate::app::input::precedence::{InputLayer, ScreenPage, input_layer};
        if input_layer(platform, renderer, frontend, match_state, persistence)
            != InputLayer::Screen(ScreenPage::Skirmish)
        {
            return None;
        }
        let (changed, next) = crate::ui::skirmish_shell::page::poll(
            &mut frontend.shell_controller,
            &mut frontend.skirmish_shell_state,
        );
        if changed {
            platform.window.request_redraw();
        }
        next
    }

    /// A release on Choose Map: the step of a button the release activates.
    #[must_use]
    pub(super) fn handle_choose_map_modal_mouse_up(
        platform: &PlatformState,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        match_state: &MatchState,
        process_assets: &ProcessAssets,
        sfx_player: &mut Option<SfxPlayer>,
    ) -> Option<ClientStep> {
        chooser(&mut frontend.skirmish_shell_state)?;
        Self::choose_map_pointer(
            platform,
            renderer,
            frontend,
            match_state,
            process_assets,
            sfx_player,
            Pointer::Up,
        )
    }

    /// A chooser button released over its own press. Every way out returns
    /// its leave as the step.
    #[must_use]
    fn choose_map_button(
        platform: &PlatformState,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        session: &MatchSession,
        fired_button: ChooseMapModalButton,
    ) -> Option<ClientStep> {
        let layout = choose_map_layout(renderer, &frontend.screen);
        let modal = frontend.skirmish_shell_state.choose_map_modal.as_mut()?;
        // Every way out slides `0x6B` out first (`0x007757E0` ->
        // `ShellDialog__SlideOutAndWait`); its result runs afterwards.
        let then: crate::app::frontend::shell_transition::ShellExitThen = match fired_button {
            // VERA: a saved map of the Random Maps folder opens in the
            // generator, which loads its seed and uses it.
            ChooseMapModalButton::UseMap0x6c5 if modal.random_maps_selected() => {
                let seed = modal.selected_saved_seed()?;
                let file = seed.file_name.clone();
                Box::new(move |state: &mut AppState| {
                    Self::commit_choose_map_random_map(
                        state.platform.game_config.as_ref(),
                        &mut state.frontend,
                        &state.process_assets,
                    );
                    Self::load_random_map_seed(
                        state.platform.game_config.as_ref(),
                        &mut state.frontend,
                        &mut state.match_state,
                        &mut state.process_assets,
                        &file,
                        true,
                    );
                })
            }
            ChooseMapModalButton::UseMap0x6c5 => {
                // No map selected: Use Map does nothing (`0x005E7160`).
                let selection = modal.accept_selection()?;
                // VERA: an Online map is downloaded first; the rest of Use
                // Map runs once it is here (`map_library`).
                if crate::app::map_library::prompt_download(platform, frontend, selection) {
                    return None;
                }
                if Self::prompt_choose_map_eject(&platform.window, frontend, session, selection) {
                    return None;
                }
                return Some(Self::leave_choose_map_for_use(
                    &mut frontend.skirmish_shell_state,
                    selection,
                ));
            }
            ChooseMapModalButton::Cancel0x5c0 => {
                Box::new(|state: &mut AppState| Self::close_choose_map_modal(&mut state.frontend))
            }
            ChooseMapModalButton::CreateRandomMap0x583 => Box::new(|state: &mut AppState| {
                Self::commit_choose_map_random_map(
                    state.platform.game_config.as_ref(),
                    &mut state.frontend,
                    &state.process_assets,
                )
            }),
            // VERA: stars or unstars the highlighted map.
            ChooseMapModalButton::Favorite => {
                if let Some((key, starred)) = modal.toggle_favorite(
                    &frontend.skirmish_modes,
                    frontend.scenario_catalog.records(),
                    &layout,
                ) {
                    crate::app::map_library::set_favorite(platform, frontend, key, starred);
                }
                platform.window.request_redraw();
                return None;
            }
        };
        Some(ClientStep::Leave(ShellSlideKind::ChooseMap, then))
    }

    /// Use Map's eject check (`0x005E72A5`): when occupied AI rows would not
    /// fit the chosen map, show `GUI:EjectAIPlayers` and report it.
    pub(super) fn prompt_choose_map_eject(
        window: &Window,
        frontend: &mut FrontendState,
        session: &MatchSession,
        selection: ChooseMapSelection,
    ) -> bool {
        if !Self::choose_map_would_eject_ai(frontend, session, selection) {
            return false;
        }
        if let Some(modal) = chooser(&mut frontend.skirmish_shell_state) {
            modal.message_box = Some(crate::ui::skirmish_shell::ChooseMapPrompt {
                selection,
                kind: ChooseMapPromptKind::EjectAi,
            });
        }
        window.request_redraw();
        true
    }

    /// Whether Use Map must ask to eject AI players: an occupied AI row at or
    /// past the chosen map's player limit (`0x005E723C` `0x005E6520`,
    /// `0x006ACCA0`).
    fn choose_map_would_eject_ai(
        frontend: &FrontendState,
        session: &MatchSession,
        selection: ChooseMapSelection,
    ) -> bool {
        let Some(limit) = selection
            .record_index
            .and_then(|record| frontend.scenario_catalog.records().get(record))
            .map(|record| record.player_capacity)
        else {
            return false;
        };
        if frontend.shell_route == ShellRoute::NetworkSetup {
            return session.network_setup().is_some_and(|launch| {
                launch
                    .opponents
                    .iter()
                    .any(|ai| i32::from(ai.raw_slot_index) >= limit.max(0))
            });
        }
        crate::ui::skirmish_shell::ai_rows_beyond_limit_occupied(
            &frontend.skirmish_shell_state.opponents,
            limit,
        )
    }

    /// Use Map's box (the two-button message box `0x120`) on the controller
    /// over the chooser, built once and kept on its rects: OK greyed while a
    /// download runs, Enter on [`ChooseMapPrompt::default_button`] and
    /// Escape on Cancel. The box and its widgets' dialog; none while no box
    /// shows.
    fn sync_choose_map_prompt(
        renderer: &RendererState,
        frontend: &mut FrontendState,
    ) -> Option<(
        crate::ui::skirmish_shell::ChooseMapPrompt,
        crate::ui::shell::modal::ShellBox,
    )> {
        use crate::ui::shell::modal::control::CANCEL;
        use vera20k_gui::{Prop, Value};
        let prompt = chooser(&mut frontend.skirmish_shell_state)?.message_box?;
        let shell_box = prompt.shell_box(String::new(), String::new(), String::new());
        let (w, h) = (
            renderer.render_width(&frontend.screen) as i32,
            renderer.render_height(&frontend.screen) as i32,
        );
        let parts = shell_box.parts(w, h, crate::ui::shell::geom::RETAIL_SCALE);
        let controller = &mut frontend.shell_controller;
        Self::raise_choose_map_prompt(controller, shell_box.dialog);
        crate::ui::shell::menu_page::sync_nodes(
            controller,
            shell_box.dialog,
            || shell_box.nodes(),
            |id| parts.rect_of(id),
            &shell_box.enabled(),
        );
        for b in &shell_box.buttons {
            let default = Value::Bool(b.id == prompt.default_button());
            controller.set_prop(shell_box.dialog, b.id, Prop::Default, default);
            let cancel = Value::Bool(b.id == CANCEL);
            controller.set_prop(shell_box.dialog, b.id, Prop::Cancel, cancel);
        }
        Some((prompt, shell_box))
    }

    /// Use Map's box `dialog` on top of the controller, over the chooser.
    pub(crate) fn raise_choose_map_prompt(
        controller: &mut crate::ui::shell::controller::DialogController,
        dialog: crate::ui::shell::descriptor::DialogResourceId,
    ) {
        if controller.top_id() != Some(dialog) {
            controller.close(dialog);
            controller.push_modal(dialog);
        }
    }

    /// A pointer message (`down`; `None` moves) while Use Map's box shows:
    /// its owner-draw buttons (`0x00612B70`) play GUIMainButtonSound on the
    /// press and answer on the release over their own press. Download takes
    /// no press while it runs. False when no box shows.
    pub(super) fn choose_map_prompt_pointer(state: &mut AppState, down: Option<bool>) -> bool {
        use crate::ui::shell::modal::control::OK;
        let Some((prompt, shell_box)) =
            Self::sync_choose_map_prompt(&state.renderer, &mut state.frontend)
        else {
            return false;
        };
        let (w, h) = (
            state.renderer.render_width(&state.frontend.screen) as i32,
            state.renderer.render_height(&state.frontend.screen) as i32,
        );
        let parts = shell_box.parts(w, h, crate::ui::shell::geom::RETAIL_SCALE);
        if let Some(id) = Self::shell_box_pointer(
            &mut state.frontend,
            &state.match_state,
            &state.process_assets,
            &mut state.audio.sfx_player,
            &shell_box,
            &parts,
            down,
        ) {
            let step = Self::answer_choose_map_prompt(
                &state.platform,
                &mut state.frontend,
                &state.match_state.session,
                prompt,
                id == OK,
            );
            if let Some(step) = step {
                Self::apply_step(state, step);
            }
            Self::drop_choose_map_prompt(&mut state.frontend, &shell_box);
            // The dialog under the box sees the pointer where it let go.
            Self::handle_skirmish_shell_mouse_move(state);
        }
        if down.is_some() {
            state.platform.window.request_redraw();
        }
        true
    }

    /// Takes the answered box's widgets off the controller.
    fn drop_choose_map_prompt(
        frontend: &mut FrontendState,
        shell_box: &crate::ui::shell::modal::ShellBox,
    ) {
        if chooser(&mut frontend.skirmish_shell_state)
            .is_none_or(|modal| modal.message_box.is_none())
        {
            frontend.shell_controller.close(shell_box.dialog);
        }
    }

    pub(super) fn handle_choose_map_prompt_mouse_down(state: &mut AppState) -> bool {
        Self::choose_map_prompt_pointer(state, Some(true))
    }

    pub(super) fn handle_choose_map_prompt_mouse_up(state: &mut AppState) -> bool {
        Self::choose_map_prompt_pointer(state, Some(false))
    }

    /// Answers Use Map's box with OK (`ok`) or Cancel. OK goes on to Use
    /// Map's close, whose leave is the returned step; for a map just
    /// downloaded it first asks to eject AI players, as Use Map does for a
    /// map that was already here. Download fetches the map, and Cancel keeps
    /// the chooser.
    #[must_use]
    fn answer_choose_map_prompt(
        platform: &PlatformState,
        frontend: &mut FrontendState,
        session: &MatchSession,
        prompt: crate::ui::skirmish_shell::ChooseMapPrompt,
        ok: bool,
    ) -> Option<ClientStep> {
        use crate::ui::skirmish_shell::ChooseMapPromptAnswer as Answer;
        let modal = chooser(&mut frontend.skirmish_shell_state);
        match prompt.answer(ok) {
            Answer::UseMap => {
                // A chooser the random-map run hid closes without a slide
                // (`0x00608070` returns while it is invisible).
                if let Some(modal) = modal {
                    modal.message_box = None;
                }
                if prompt.kind == ChooseMapPromptKind::Downloaded
                    && Self::prompt_choose_map_eject(
                        &platform.window,
                        frontend,
                        session,
                        prompt.selection,
                    )
                {
                    return None;
                }
                Some(Self::leave_choose_map_for_use(
                    &mut frontend.skirmish_shell_state,
                    prompt.selection,
                ))
            }
            Answer::Download => {
                crate::app::map_library::start_download(platform, frontend, prompt.selection);
                None
            }
            Answer::Close => {
                if let Some(modal) = modal {
                    modal.close_prompt();
                }
                crate::app::map_library::cancel_download(frontend);
                None
            }
            Answer::Wait => None,
        }
    }

    /// VERA: a printable key or Backspace while Choose Map shows edits its
    /// search line. False for other keys, and while a box or the random-map
    /// run is over the chooser.
    pub(super) fn choose_map_search_key(
        platform: &PlatformState,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        match_state: &MatchState,
        persistence: &PersistenceState,
        event: &winit::event::KeyEvent,
    ) -> bool {
        if !event.state.is_pressed() {
            return false;
        }
        if event.physical_key == PhysicalKey::Code(KeyCode::Backspace) {
            let backspace = vera20k_gui::InputEvent::Key(vera20k_gui::Key::Backspace);
            return Self::choose_map_search_input(
                platform,
                renderer,
                frontend,
                match_state,
                persistence,
                backspace,
            );
        }
        let typed = event.text.as_deref().unwrap_or_default();
        typed.chars().any(|ch| !ch.is_control())
            && Self::choose_map_search_text(
                platform,
                renderer,
                frontend,
                match_state,
                persistence,
                typed,
            )
    }

    /// VERA's Esc menu (`app::shell_menu`): Esc stays with the seed browser,
    /// a validation box, the Game Options box and a chooser's prompt, and
    /// does nothing while a random map generates.
    pub(super) fn skirmish_leaves_escape(skirmish_shell: &SkirmishShellState) -> bool {
        let shell = skirmish_shell;
        shell.saved_seed_browser.is_none()
            && shell.setups_browser.is_none()
            && shell.validation_modal.is_none()
            && shell.game_options.is_none()
            && shell
                .choose_map_modal
                .as_ref()
                .is_none_or(|modal| modal.message_box.is_none())
            && shell
                .random_map_setup_modal
                .as_ref()
                .is_none_or(|modal| !modal.generating)
    }

    /// Enter or Escape on Use Map's box, through its widgets: Enter its
    /// default button ([`ChooseMapPrompt::default_button`]), Escape Cancel.
    /// While the download runs, Enter does nothing and Escape cancels.
    pub(super) fn answer_choose_map_prompt_key(state: &mut AppState, enter: bool) -> bool {
        use crate::ui::shell::modal::control::OK;
        use vera20k_gui::{InputEvent, Key};
        let Some((prompt, shell_box)) =
            Self::sync_choose_map_prompt(&state.renderer, &mut state.frontend)
        else {
            return false;
        };
        let key = if enter { Key::Enter } else { Key::Escape };
        let controller = &mut state.frontend.shell_controller;
        controller.on_input(InputEvent::Key(key));
        let raw = controller.drain_events();
        let events = crate::ui::shell::page::page_events(controller.gui(), raw);
        let answer = events.into_iter().find_map(|event| match event {
            crate::ui::shell::page::PageEvent::Activated(id) => Some(id),
            _ => None,
        });
        if let Some(id) = answer {
            let step = Self::answer_choose_map_prompt(
                &state.platform,
                &mut state.frontend,
                &state.match_state.session,
                prompt,
                id == OK,
            );
            if let Some(step) = step {
                Self::apply_step(state, step);
            }
            Self::drop_choose_map_prompt(&mut state.frontend, &shell_box);
        }
        state.platform.window.request_redraw();
        true
    }

    /// Use Map, after `0x6B`'s slide-out (none while the random-map run
    /// hides it): commit the selection, close the chooser; `0x102` shows
    /// again and replays its entry slide. A selection that cannot commit
    /// keeps the chooser, which shows again if it was hidden.
    /// Use Map's slide-out: the selection waits in the chooser
    /// ([`crate::ui::skirmish_shell::ChooseMapModalState::use_selection`]),
    /// where a catalog change during the slide remaps it, and is committed
    /// once the slide ends. The slide is the returned step.
    fn leave_choose_map_for_use(
        skirmish_shell: &mut SkirmishShellState,
        selection: ChooseMapSelection,
    ) -> ClientStep {
        if let Some(modal) = chooser(skirmish_shell) {
            modal.use_selection = Some(selection);
        }
        ClientStep::Leave(
            ShellSlideKind::ChooseMap,
            Box::new(|state: &mut AppState| {
                if let Some(selection) = chooser(&mut state.frontend.skirmish_shell_state)
                    .and_then(|modal| modal.use_selection.take())
                {
                    App::commit_choose_map_use(
                        state.platform.game_config.as_ref(),
                        &mut state.frontend,
                        &mut state.match_state,
                        &state.process_assets,
                        selection,
                    );
                }
            }),
        )
    }

    pub(super) fn commit_choose_map_use(
        game_config: Option<&GameConfig>,
        frontend: &mut FrontendState,
        match_state: &mut MatchState,
        process_assets: &ProcessAssets,
        selection: ChooseMapSelection,
    ) {
        if Self::commit_choose_map_selection(
            game_config,
            frontend,
            match_state,
            process_assets,
            selection,
        ) {
            Self::close_choose_map_modal(frontend);
        } else if let Some(modal) = chooser(&mut frontend.skirmish_shell_state) {
            modal.show();
        }
    }

    /// `0x6B`'s slide-out has run for Create Random Map: the chooser hides
    /// (`0x005E6A0B`) while the random-map dialog `0x105` runs.
    pub(super) fn commit_choose_map_random_map(
        game_config: Option<&GameConfig>,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
    ) {
        let Some(modal) = chooser(&mut frontend.skirmish_shell_state) else {
            return;
        };
        modal.hide();
        let previous = modal.cancel_selection();
        // Ares.dll is loaded when the game is read the Ares way: it reads
        // the urban-areas setting, and the dialog lists Ares' entries and
        // shows its Urban Areas box.
        let ares = process_assets.reads_ares_way();
        if ares {
            let settings = process_assets
                .manager()
                .map(crate::map::rmg::RmgSettings::load)
                .unwrap_or_default();
            frontend
                .multiplayer_shell_runtime
                .read_generate_urban_areas_once(&settings);
        }
        let options = frontend
            .multiplayer_shell_runtime
            .random_map_options_for_setup();
        let available = Self::random_map_dir(game_config)
            .is_some_and(|dir| crate::map::rmg::saved_seeds::saved_seeds_available(&dir));
        let mut modal = crate::ui::skirmish_shell::RandomMapSetupModalState::open(
            options,
            Some(previous),
            available,
        );
        if frontend.shell_route == ShellRoute::NetworkSetup {
            modal.use_network_player_limit();
        }
        modal.ares = ares;
        // VERA: the player's own presets, for the Presets button.
        if let Some(dir) = Self::random_map_dir(game_config) {
            modal.custom_presets = crate::app::shell_random_map::load_custom_presets(&dir, ares);
        }
        frontend.skirmish_shell_state.random_map_setup_modal = Some(modal);
    }

    /// The name box `0x6A0` loses the keyboard focus.
    pub(super) fn blur_player_name(controller: &mut DialogController) {
        let page = crate::ui::skirmish_shell::page::DIALOG;
        controller.focus_control(page, None);
    }

    /// `0x102`'s proc runs the common handler first (`0x006AE40A`): every
    /// move writes the help, which repaints the status line. A trackbar
    /// holding the mouse keeps the dialog's hit test (`WM_NCHITTEST`,
    /// `0x00622CCB`) from running until the release.
    fn update_skirmish_shell_status_help(
        window: &Window,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        x: i32,
        y: i32,
    ) {
        use crate::ui::skirmish_shell::{SkirmishHoverTarget, page};
        let shell = &frontend.skirmish_shell_state;
        let gui = frontend.shell_controller.gui();
        if page::pointer_held(gui) {
            return;
        }
        let maps = frontend.scenario_catalog.shell_maps();
        let root = frontend.shell_controller.root_in(page::DIALOG);
        let target = page::hover_target(gui, root, shell, maps, x, y);
        let key = target.and_then(|target| {
            crate::ui::skirmish_shell::status_help_key_for_hover_with_catalog(
                target,
                Some(&shell.country_catalog),
            )
        });
        let preview = target == Some(SkirmishHoverTarget::MapPreview0x468);
        let text = Self::status_help(frontend, process_assets, key, preview);
        if (frontend.skirmish_shell_state.statics).hover(&text, Instant::now()) {
            window.request_redraw();
        }
    }

    /// The status line's text for help `key`; over a map `preview` it says
    /// that a press enlarges it.
    fn status_help(
        frontend: &FrontendState,
        process_assets: &ProcessAssets,
        key: Option<&str>,
        preview: bool,
    ) -> String {
        let text = key
            .map(|key| process_assets.csf_label(key, ""))
            .unwrap_or_default();
        match preview {
            true => crate::app::frontend::preview_zoom::box_help(
                &frontend.skirmish_preview_texture,
                text,
            ),
            false => text,
        }
    }

    /// A child's mouse move writes its help; over the background the last
    /// text stays (`0x6B` has no clearing hit test). Over a game-type row the
    /// help is the mode's description (`0x4E9`, `0x005E6E44`).
    fn update_choose_map_modal_status_help(
        window: &Window,
        frontend: &mut FrontendState,
        input: &MatchInputState,
        process_assets: &ProcessAssets,
    ) {
        use crate::ui::skirmish_shell::ChooseMapHoverTarget as Target;
        let (x, y) = input.cursor_px();
        let gui = frontend.shell_controller.gui();
        let modal = frontend.skirmish_shell_state.choose_map_modal.as_ref();
        let Some(target) = modal.and_then(|modal| {
            crate::ui::skirmish_shell::choose_map_page::hover_target(gui, modal, x, y)
        }) else {
            return;
        };
        let mode_help = match target {
            Target::ModeListRow0x6eb { mode } => mode
                .resolve(&frontend.skirmish_modes)
                .filter(|mode| !mode.tooltip_key.is_empty())
                .map(|mode| process_assets.csf_label(&mode.tooltip_key, ""))
                .filter(|text| !text.is_empty()),
            _ => None,
        };
        let text = mode_help.unwrap_or_else(|| {
            let key = crate::ui::skirmish_shell::status_help_key_for_choose_map_hover(target);
            Self::status_help(
                frontend,
                process_assets,
                key,
                target == Target::Preview0x468,
            )
        });
        // A child's hover message writes and repaints the status line.
        if let Some(modal) = chooser(&mut frontend.skirmish_shell_state)
            && modal.statics.hover(&text, Instant::now())
        {
            window.request_redraw();
        }
    }

    pub(super) fn handle_skirmish_shell_key_input(
        platform: &PlatformState,
        bit_font: &BitFont,
        frontend: &mut FrontendState,
        code: KeyCode,
        text: Option<&str>,
    ) -> bool {
        let shift = platform.live_modifiers.shift_key();
        let event = crate::ui::shell::controller::gui_input(Some(code), shift, text);
        let font = bit_font;
        let focused = crate::ui::skirmish_shell::page::key(
            &mut frontend.shell_controller,
            &mut frontend.skirmish_shell_state,
            event,
            |text| font.text_width(text),
        );
        if focused {
            platform.window.request_redraw();
        }
        focused
    }

    fn close_validation_modal_from_controller(frontend: &mut FrontendState) {
        frontend.skirmish_shell_state.validation_modal = None;
        if frontend.shell_controller.top_id() == Some(VALIDATION_BOX) {
            frontend.shell_controller.pop();
        }
    }

    /// A key while `0xCE` is up: Enter and Escape reach its hidden IDOK,
    /// which closes it as OK does.
    pub(super) fn route_validation_modal_key(
        window: &Window,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        match_state: &MatchState,
        process_assets: &ProcessAssets,
        sfx_player: &mut Option<SfxPlayer>,
        code: KeyCode,
    ) -> bool {
        let Some(shell_box) = frontend.skirmish_shell_state.validation_modal.clone() else {
            return false;
        };
        let Some(key) = crate::ui::shell::controller::gui_key(code, false) else {
            return false;
        };
        let (w, h) = (
            renderer.render_width(&frontend.screen) as i32,
            renderer.render_height(&frontend.screen) as i32,
        );
        let parts = shell_box.parts(w, h, crate::ui::shell::geom::RETAIL_SCALE);
        frontend.shell_controller.ensure_active(VALIDATION_BOX);
        let event = vera20k_gui::InputEvent::Key(key);
        if Self::shell_box_input(
            frontend,
            match_state,
            process_assets,
            sfx_player,
            &shell_box,
            &parts,
            event,
        )
        .is_none()
        {
            return false;
        }
        Self::close_validation_modal_from_controller(frontend);
        window.request_redraw();
        true
    }

    /// A press or release while `0xCE` is up: its owner-draw OK plays
    /// GUIMainButtonSound on the press (`0x00612B70` via the message box
    /// `0x005D3490`) and closes the box on its release.
    fn route_validation_modal_mouse(
        window: &Window,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        match_state: &MatchState,
        process_assets: &ProcessAssets,
        sfx_player: &mut Option<SfxPlayer>,
        down: bool,
    ) -> bool {
        use crate::ui::shell::modal::control::OK;
        let Some(shell_box) = frontend.skirmish_shell_state.validation_modal.clone() else {
            return false;
        };
        let (w, h) = (
            renderer.render_width(&frontend.screen) as i32,
            renderer.render_height(&frontend.screen) as i32,
        );
        let parts = shell_box.parts(w, h, crate::ui::shell::geom::RETAIL_SCALE);
        frontend.shell_controller.ensure_active(shell_box.dialog);
        if Self::shell_box_pointer(
            frontend,
            match_state,
            process_assets,
            sfx_player,
            &shell_box,
            &parts,
            Some(down),
        ) == Some(OK)
        {
            Self::close_validation_modal_from_controller(frontend);
        }
        window.request_redraw();
        true
    }

    /// `0x102`'s widgets under the shell controller, built once per visit and
    /// showing the session; their root.
    pub(crate) fn sync_skirmish_page(
        renderer: &RendererState,
        frontend: &mut FrontendState,
        match_state: &MatchState,
        process_assets: &ProcessAssets,
    ) -> Option<vera20k_gui::NodeId> {
        let layout = skirmish_layout(renderer, frontend);
        let labels = crate::app::frontend::skirmish_shell_render::skirmish_labels(
            frontend,
            match_state,
            process_assets,
        );
        crate::ui::skirmish_shell::page::sync_page(
            &mut frontend.shell_controller,
            &frontend.skirmish_shell_state,
            &layout,
            frontend.scenario_catalog.shell_maps(),
            &labels,
        )
    }

    /// The pointer on `0x102`'s widgets: the session takes their events, their
    /// cues play and an activated button acts; its step is returned.
    #[must_use]
    fn skirmish_page_pointer(
        platform: &PlatformState,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        match_state: &MatchState,
        process_assets: &mut ProcessAssets,
        sfx_player: &mut Option<SfxPlayer>,
        options_profile: &RetailOptionsProfile,
        at: Pointer,
    ) -> Option<ClientStep> {
        let layout = skirmish_layout(renderer, frontend);
        let labels = crate::app::frontend::skirmish_shell_render::skirmish_labels(
            frontend,
            match_state,
            process_assets,
        );
        let (x, y) = match_state.input.cursor_px();
        let (cues, action) = crate::ui::skirmish_shell::page::pointer(
            &mut frontend.shell_controller,
            &mut frontend.skirmish_shell_state,
            &layout,
            frontend.scenario_catalog.shell_maps(),
            &labels,
            at,
            x,
            y,
        );
        Self::play_cues(frontend, match_state, process_assets, sfx_player, cues);
        // A pick from Quick Team Setup or Bulk Seat Actions acts on the rows.
        Self::run_skirmish_menu_pick(platform, frontend, match_state, process_assets);
        platform.window.request_redraw();
        let action = action?;
        Self::handle_skirmish_shell_action(
            &platform.window,
            renderer,
            frontend,
            process_assets,
            options_profile,
            action,
        )
    }

    /// Whether the page's widgets have a list open.
    fn skirmish_list_open(controller: &DialogController) -> bool {
        controller.top_id() == Some(crate::ui::skirmish_shell::page::DIALOG)
            && controller.gui().popup().is_some()
    }

    /// Closes the page's open list, without a cue.
    pub(super) fn close_skirmish_list(controller: &mut DialogController) -> bool {
        Self::skirmish_list_open(controller) && controller.gui_mut().close_popup()
    }

    /// The dialog on top of the Skirmish stack when it is not `0x102`.
    fn skirmish_page_covered(state: &mut AppState, at: Pointer) -> bool {
        let down = match at {
            Pointer::Move => None,
            at => Some(at == Pointer::Down),
        };
        match state.frontend.skirmish_shell_state.top_dialog() {
            Some(SkirmishShellDialog::Skirmish) => {
                return Self::route_game_options_mouse(
                    &state.platform,
                    &state.renderer,
                    &mut state.frontend,
                    &state.match_state,
                    &mut state.process_assets,
                    &mut state.audio.sfx_player,
                    &mut state.persistence.options_profile,
                    at,
                );
            }
            Some(SkirmishShellDialog::SeedBrowser) => match down {
                Some(down) => {
                    _ = Self::saved_seed_browser_pointer(
                        state.platform.game_config.as_ref(),
                        &state.renderer,
                        &mut state.frontend,
                        &mut state.match_state,
                        &mut state.process_assets,
                        &mut state.audio.sfx_player,
                        Some(down),
                    )
                }
                None => Self::update_saved_seed_browser_scroll(
                    state.platform.game_config.as_ref(),
                    &state.renderer,
                    &mut state.frontend,
                    &mut state.match_state,
                    &mut state.process_assets,
                    &mut state.audio.sfx_player,
                    true,
                ),
            },
            Some(SkirmishShellDialog::Setups) => match down {
                Some(down) => {
                    Self::skirmish_setups_pointer(
                        &state.platform.window,
                        &state.renderer,
                        &mut state.frontend,
                        &state.match_state,
                        &state.process_assets,
                        &mut state.audio.sfx_player,
                        Some(down),
                    );
                    if down {
                        Self::update_skirmish_setups_scroll(
                            &state.platform.window,
                            &state.renderer,
                            &mut state.frontend,
                            &state.match_state,
                            &state.process_assets,
                            &mut state.audio.sfx_player,
                            false,
                        );
                    }
                }
                None => Self::update_skirmish_setups_scroll(
                    &state.platform.window,
                    &state.renderer,
                    &mut state.frontend,
                    &state.match_state,
                    &state.process_assets,
                    &mut state.audio.sfx_player,
                    true,
                ),
            },
            Some(SkirmishShellDialog::RandomMap) => match down {
                Some(true) => {
                    _ = Self::handle_random_map_setup_mouse_down(
                        &state.platform.window,
                        &state.renderer,
                        &mut state.frontend,
                        &state.match_state,
                        &state.process_assets,
                        &mut state.audio.sfx_player,
                    )
                }
                Some(false) => {
                    let steps = Self::handle_random_map_setup_mouse_up(
                        &state.platform,
                        &state.renderer,
                        &mut state.frontend,
                        &mut state.match_state,
                        &mut state.process_assets,
                        &mut state.audio.sfx_player,
                    );
                    for step in steps {
                        Self::apply_step(state, step);
                    }
                }
                None => Self::handle_random_map_setup_mouse_move(
                    &state.platform.window,
                    &state.renderer,
                    &mut state.frontend,
                    &state.match_state,
                    &state.process_assets,
                    &mut state.audio.sfx_player,
                ),
            },
            Some(SkirmishShellDialog::ChooseMap) => match down {
                Some(true) => {
                    let (_, step) = Self::handle_choose_map_modal_mouse_down(
                        &state.platform,
                        &state.renderer,
                        &mut state.frontend,
                        &state.match_state,
                        &state.process_assets,
                        &mut state.audio.sfx_player,
                    );
                    if let Some(step) = step {
                        Self::apply_step(state, step);
                    }
                }
                Some(false) => {
                    let step = Self::handle_choose_map_modal_mouse_up(
                        &state.platform,
                        &state.renderer,
                        &mut state.frontend,
                        &state.match_state,
                        &state.process_assets,
                        &mut state.audio.sfx_player,
                    );
                    if let Some(step) = step {
                        Self::apply_step(state, step);
                    }
                }
                None => {
                    let step = Self::choose_map_pointer(
                        &state.platform,
                        &state.renderer,
                        &mut state.frontend,
                        &state.match_state,
                        &state.process_assets,
                        &mut state.audio.sfx_player,
                        Pointer::Move,
                    );
                    if let Some(step) = step {
                        Self::apply_step(state, step);
                    }
                    Self::update_choose_map_modal_status_help(
                        &state.platform.window,
                        &mut state.frontend,
                        &state.match_state.input,
                        &state.process_assets,
                    );
                }
            },
            None => {}
        }
        true
    }

    pub(super) fn handle_skirmish_shell_mouse_down(state: &mut AppState) {
        // A message box runs over the dialogs; else the dialog on top.
        if Self::route_validation_modal_mouse(
            &state.platform.window,
            &state.renderer,
            &mut state.frontend,
            &state.match_state,
            &state.process_assets,
            &mut state.audio.sfx_player,
            true,
        ) || Self::handle_choose_map_prompt_mouse_down(state)
            || Self::skirmish_page_covered(state, Pointer::Down)
        {
            return;
        }
        let (x, y) = state.match_state.input.cursor_px();
        if crate::ui::skirmish_shell::page::over_name(&state.frontend.shell_controller, x, y) {
            // The name box takes the press even over an open list, which
            // closes without its cue; the press selects the name.
            Self::close_skirmish_list(&mut state.frontend.shell_controller);
            let step = Self::skirmish_page_pointer(
                &state.platform,
                &state.renderer,
                &mut state.frontend,
                &state.match_state,
                &mut state.process_assets,
                &mut state.audio.sfx_player,
                &state.persistence.options_profile,
                Pointer::Down,
            );
            if let Some(step) = step {
                Self::apply_step(state, step);
            }
            let font = &state.renderer.bit_font;
            crate::ui::skirmish_shell::page::scroll_player_name(
                &mut state.frontend.shell_controller,
                |text| font.text_width(text),
            );
            return;
        }
        let over = crate::app::frontend::preview_zoom::over_box(
            &state.frontend.shell_controller,
            &state.match_state.input,
            crate::ui::skirmish_shell::page::DIALOG,
        );
        let zoom = over && !Self::skirmish_list_open(&state.frontend.shell_controller);
        // The widgets take the press (the name box loses the focus to it);
        // on the preview it takes nothing and enlarges the picture.
        let step = Self::skirmish_page_pointer(
            &state.platform,
            &state.renderer,
            &mut state.frontend,
            &state.match_state,
            &mut state.process_assets,
            &mut state.audio.sfx_player,
            &state.persistence.options_profile,
            Pointer::Down,
        );
        if let Some(step) = step {
            Self::apply_step(state, step);
        }
        if zoom {
            crate::app::frontend::preview_zoom::press_on_box(
                &mut state.frontend,
                &state.match_state.input,
                true,
            );
        }
    }

    pub(super) fn handle_skirmish_shell_mouse_up(state: &mut AppState) {
        if Self::route_validation_modal_mouse(
            &state.platform.window,
            &state.renderer,
            &mut state.frontend,
            &state.match_state,
            &state.process_assets,
            &mut state.audio.sfx_player,
            false,
        ) || Self::handle_choose_map_prompt_mouse_up(state)
            || Self::skirmish_page_covered(state, Pointer::Up)
        {
            return;
        }
        // A release after a trackbar or scrollbar press goes to that control,
        // which holds the mouse; freeing it lets the dialog's hit test run
        // again.
        let held =
            crate::ui::skirmish_shell::page::pointer_held(state.frontend.shell_controller.gui());
        let step = Self::skirmish_page_pointer(
            &state.platform,
            &state.renderer,
            &mut state.frontend,
            &state.match_state,
            &mut state.process_assets,
            &mut state.audio.sfx_player,
            &state.persistence.options_profile,
            Pointer::Up,
        );
        if let Some(step) = step {
            Self::apply_step(state, step);
        }
        if held {
            let (x, y) = state.match_state.input.cursor_px();
            Self::update_skirmish_shell_status_help(
                &state.platform.window,
                &mut state.frontend,
                &state.process_assets,
                x,
                y,
            );
        }
    }

    pub(super) fn handle_skirmish_shell_mouse_move(state: &mut AppState) {
        if Self::choose_map_prompt_pointer(state, None)
            || Self::skirmish_page_covered(state, Pointer::Move)
        {
            return;
        }
        // Under the validation box the page takes no move and the status
        // line clears.
        if state
            .frontend
            .skirmish_shell_state
            .validation_modal
            .is_none()
        {
            let step = Self::skirmish_page_pointer(
                &state.platform,
                &state.renderer,
                &mut state.frontend,
                &state.match_state,
                &mut state.process_assets,
                &mut state.audio.sfx_player,
                &state.persistence.options_profile,
                Pointer::Move,
            );
            if let Some(step) = step {
                Self::apply_step(state, step);
            }
        }
        let (x, y) = state.match_state.input.cursor_px();
        Self::update_skirmish_shell_status_help(
            &state.platform.window,
            &mut state.frontend,
            &state.process_assets,
            x,
            y,
        );
    }

    pub(super) fn handle_skirmish_shell_mouse_wheel(
        renderer: &RendererState,
        frontend: &mut FrontendState,
        match_state: &MatchState,
        process_assets: &ProcessAssets,
        lines: f32,
    ) -> bool {
        let shell = &frontend.skirmish_shell_state;
        // The chooser's lists have no wheel case (`0x00618D40`).
        if shell.saved_seed_browser.is_some()
            || shell.setups_browser.is_some()
            || shell.validation_modal.is_some()
            || shell.game_options.is_some()
            || shell.choose_map_modal.is_some()
        {
            return true;
        }
        let layout = skirmish_layout(renderer, frontend);
        let labels = crate::app::frontend::skirmish_shell_render::skirmish_labels(
            frontend,
            match_state,
            process_assets,
        );
        crate::ui::skirmish_shell::page::wheel(
            &mut frontend.shell_controller,
            &mut frontend.skirmish_shell_state,
            &layout,
            frontend.scenario_catalog.shell_maps(),
            &labels,
            lines,
        )
    }

    /// Plays a dialog's cues in order.
    pub(super) fn play_cues(
        frontend: &FrontendState,
        match_state: &MatchState,
        process_assets: &ProcessAssets,
        sfx_player: &mut Option<SfxPlayer>,
        cues: Vec<PageCue>,
    ) {
        for cue in cues {
            Self::play_page_cue(frontend, match_state, process_assets, sfx_player, cue);
        }
    }
}

#[cfg(all(test, tests_app))]
mod tests {
    #[cfg(feature = "verax")]
    use super::*;

    /// A loose map the chooser lists: the retail `.yrm` branch reads
    /// `[Basic] Name=` from the file itself.
    #[cfg(feature = "verax")]
    fn write_loose_map(dir: &std::path::Path, file_name: &str, display_name: &str) {
        std::fs::write(
            dir.join(file_name),
            format!("[Basic]\nName={display_name}\nGameMode=standard\n"),
        )
        .expect("write loose map");
    }

    #[cfg(feature = "verax")]
    fn synthetic_record(file_name: &str) -> crate::map::skirmish_scenarios::SkirmishScenarioRecord {
        let ini =
            crate::rules::ini_parser::IniFile::from_str(&format!("[Basic]\nName={file_name}\n"));
        crate::map::skirmish_scenarios::SkirmishScenarioRecord::concrete_from_ini(
            0,
            crate::map::skirmish_scenarios::SkirmishScenarioSource::LooseMap(file_name.to_string()),
            file_name,
            &ini,
        )
    }

    /// VeraX map hot reload: the app rescans the same process manager when the
    /// chooser opens, so a map copied after launch must appear without a
    /// restart and without duplicating the rows already listed.
    #[cfg(feature = "verax")]
    #[test]
    fn rescan_lists_a_map_copied_after_launch_without_duplicating_rows() {
        use crate::assets::asset_manager::test_support::audio_test_install;
        use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};

        let install = audio_test_install("verax-map-reload");
        write_loose_map(install.path(), "ALPHA.YRM", "Alpha");
        let mut assets = AssetManager::new(install.path(), MediaArchiveMode::STOCK_DIGITAL)
            .expect("synthetic install");

        let launch =
            App::scan_skirmish_records(install.path(), &mut assets, None).expect("launch scan");
        assert_eq!(
            launch
                .iter()
                .filter(|record| record.file_name.eq_ignore_ascii_case("ALPHA.YRM"))
                .count(),
            1
        );

        write_loose_map(install.path(), "BRAVO.YRM", "Bravo");
        let reloaded = App::scan_skirmish_records(install.path(), &mut assets, None)
            .expect("rescan through the same manager");
        assert_eq!(
            reloaded
                .iter()
                .filter(|record| record.file_name.eq_ignore_ascii_case("ALPHA.YRM"))
                .count(),
            1,
            "rescanning must not duplicate the launch rows"
        );
        assert_eq!(
            reloaded
                .iter()
                .filter(|record| record.file_name.eq_ignore_ascii_case("BRAVO.YRM"))
                .count(),
            1,
            "the copied map is listed without restarting"
        );
    }

    /// The chooser's selection is a list index; the rescan may move it, so the
    /// selected file name must win over the stale index.
    #[cfg(feature = "verax")]
    #[test]
    fn selection_follows_the_file_not_the_stale_index() {
        let records = [
            synthetic_record("ALPHA.YRM"),
            synthetic_record("BRAVO.YRM"),
            synthetic_record("CHARLIE.YRM"),
        ];
        assert_eq!(
            App::remap_record_index(Some("CHARLIE.YRM"), Some(0), &records),
            Some(2)
        );
        assert_eq!(App::remap_record_index(None, Some(2), &records), Some(2));

        let shorter = [synthetic_record("ALPHA.YRM"), synthetic_record("BRAVO.YRM")];
        assert_eq!(
            App::remap_record_index(Some("CHARLIE.YRM"), Some(2), &shorter),
            Some(1),
            "a removed map falls back to a surviving index"
        );
        assert_eq!(
            App::remap_record_index(Some("BRAVO.YRM"), Some(0), &[]),
            None
        );
    }
}
