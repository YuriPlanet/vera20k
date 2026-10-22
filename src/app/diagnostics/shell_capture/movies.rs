//! Production-only main-menu family capture routes (Movies & Credits, the
//! Exit confirmation and campaign selection). Route actions go through the
//! ordinary main-menu, `0x100`, `0x101`, `0x129` and `0x94` handlers after a
//! presented frame; no shell state or renderer is cloned.

use super::*;
use crate::app::App;
use crate::ui::campaign_shell::CampaignSide;
use serde_json::{Value, json};

mod abort_return;
mod campaign_restart;
pub(super) use campaign_restart::RestartTarget;
mod lan;
pub(crate) use lan::{LAN_CHECKPOINTS, LanTarget};
mod wol;
pub(crate) use wol::{WOL_CHECKPOINTS, WolTarget};
mod network_options;
mod opening;
pub(crate) use network_options::NetworkOptionsTarget;
pub(super) use opening::OpeningTarget;

/// Native control corpus read from this checkout's `tools/` at capture time.
/// Diagnostics run from a development checkout, so the release binary does
/// not embed the corpus (`architecture_guards` keeps fixtures out of builds).
fn native_corpus(path: &str) -> Result<Value> {
    let full = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
    let text = std::fs::read_to_string(&full)
        .with_context(|| format!("cannot read native corpus {}", full.display()))?;
    Ok(serde_json::from_str(&text)?)
}

/// Row-0 press point for the selected-list checkpoint (inside `0x744`).
const LIST_ROW0_POINT: (f32, f32) = (150.0, 137.0);
/// Down-arrow press point of the 17-row list's scrollbar.
const LIST_DOWN_ARROW_POINT: (f32, f32) = (509.0, 420.0);
/// Frames to present after the route settles, so reveals and pending state
/// changes show before the readback.
const SETTLE_FRAMES: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MoviesTarget {
    Page0x101,
    List0x129,
    List0x129Selected,
    /// Every campaign movie unlocked (diagnostic profile override), after
    /// `down_presses` down-arrow presses.
    FullList {
        down_presses: u8,
    },
    Credits {
        frame: u64,
    },
    SneakPeek {
        frame: usize,
    },
    /// Exit Game through the production teardown slide, then state 6.
    ExitConfirm,
    /// A teardown slide held at `tick`: Exit Game on `0xE2` or Back on
    /// `0x129`.
    SlideOut {
        kind: ShellSlideKind,
        tick: u32,
    },
    /// Back on `0x129`, read back on the first frame after its teardown: the
    /// recreated `0x101` must already show its entry slide at tick 0.
    ListBackFirstFrame,
    /// Single Player -> New Campaign: dialog `0x94` settled, optionally after a
    /// press on the difficulty slider at `press` (still held, with the pointer
    /// moved to `hold_over`, when that is set), or held at `entry_tick` of its
    /// entry slide.
    Campaign0x94 {
        press: Option<(i32, i32)>,
        hold_over: Option<(i32, i32)>,
        entry_tick: Option<u32>,
    },
    /// Normal campaign emblem admission, through the movie/loading continuation.
    CampaignStart {
        side: CampaignSide,
        target: CampaignStartTarget,
    },
    /// Back on the settled campaign page, then the recreated Single Player page.
    CampaignBack,
    /// Single Player -> Load Saved Game: dialog `0xB7` settled, or held at
    /// `entry_tick` of its entry slide.
    LoadSavedGame0xB7 {
        entry_tick: Option<u32>,
    },
    /// Main Menu -> Options: dialog `0xD5` settled, or held at `entry_tick`
    /// of its entry slide.
    Options0xD5 {
        entry_tick: Option<u32>,
        /// Rest the pointer here instead of at the neutral point.
        hover: Option<(i32, i32)>,
        /// The page captured: VERA's is reached by pressing Vera Options on
        /// the settled main page.
        page: crate::ui::main_menu_dialogs::options::OptionsPage,
    },
    /// Network on `0xE2`: its teardown slide, then a new `0xE2` with its
    /// own entry slide, captured once settled.
    NetworkBounce,
    NetworkOptions(NetworkOptionsTarget),
    Lan(LanTarget),
    Wol(WolTarget),
    /// Main Menu -> Internet: `0x10E` settled (pointer at `hover` or
    /// neutral), held at `entry_tick`, or after one of its buttons.
    WolWelcome {
        entry_tick: Option<u32>,
        hover: Option<(i32, i32)>,
        press: WolPress,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CampaignStartTarget {
    LoadingFirst,
    FirstLive,
    InitialSaved,
    /// Explicit-map CnCNet admission, without playing preceding missions.
    ExplicitFirstLive(&'static str),
    AbortReturn,
    Opening(OpeningTarget),
    Restart(RestartTarget),
}

impl CampaignStartTarget {
    fn explicit_scenario(self) -> Option<&'static str> {
        match self {
            Self::ExplicitFirstLive(scenario) => Some(scenario),
            Self::Opening(OpeningTarget::RadarAlert | OpeningTarget::RadarRestored) => {
                Some("SOV02SMD.MAP")
            }
            Self::Restart(RestartTarget::ObjectiveExpiry) => Some("SOV02SMD.MAP"),
            Self::Restart(RestartTarget::AlternateHomeVictory) => Some("SOV06LMD.MAP"),
            Self::Restart(RestartTarget::InheritedTimerVictory) => Some("SOV01UMD.MAP"),
            _ => None,
        }
    }
}

/// A `0x10E` button the capture presses once the page has settled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WolPress {
    None,
    /// My Information: `0x10E` slides out and the `TXT_APIMISSING` box shows.
    MyInformation,
    /// Main Menu: `0x10E` slides out and a new `0xE2` settles.
    MainMenu,
}

/// Client points of the pressed buttons at 800x600.
const WOL_MY_INFORMATION_POINT: (i32, i32) = (720, 388);
const WOL_MAIN_MENU_POINT: (i32, i32) = (720, 556);

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Phase {
    #[default]
    MainMenu,
    Page,
    List,
    Credits,
    Movie,
    Exit,
    SlideOut,
    SinglePlayerPage,
    Campaign,
    /// The emblem press has been presented; the next frame releases it.
    CampaignPressed,
    CampaignStarting,
    CampaignInitialSaved,
    CampaignReturning,
    CampaignAbortMenu,
    CampaignLeaving,
    /// The restart driver owns the route from the first live frame.
    CampaignRestart,
    LoadSavedGame,
    Options,
    NetworkOptions,
    NetworkOptionsInput,
    NetworkOptionsMessage,
    NetworkOptionsPortWarning,
    NetworkOptionsReturning,
    NetworkOptionsMainMenu,
    /// Network pressed: waiting for the teardown to commit.
    NetworkLeaving,
    /// A new `0xE2` exists: waiting for its entry slide to be seen.
    NetworkReturning,
    /// The new `0xE2`'s entry slide was seen: waiting for steady paint.
    NetworkReturned,
    Wol,
    /// A `0x10E` button was pressed: waiting for its teardown and result.
    WolLeaving,
    Settling(u32),
}

pub(super) struct MoviesCapture {
    target: MoviesTarget,
    phase: Phase,
    route: Vec<Value>,
    /// A read-only receipt of the ordinary LoadingRequest, observed before it
    /// retires. This does not supply inputs to the loader or simulation.
    observed_campaign_startup: Option<Value>,
    /// Abort-return observes the installed owners once before opening B5.
    observed_campaign_runtime: Option<Value>,
    observed_exit_cascade: bool,
    abort_input: Option<abort_return::AbortReturnInput>,
    restart: Option<campaign_restart::RestartCapture>,
    restart_save_directory: PathBuf,
    saves_isolated: bool,
    opening: Option<opening::OpeningCapture>,
    lan: Option<lan::LanCapture>,
    wol: Option<wol::WolCapture>,
}

impl MoviesCapture {
    pub(super) fn new(
        target: MoviesTarget,
        output: &Path,
        live_lan: Option<super::LiveLanConfig>,
    ) -> Self {
        Self {
            target,
            lan: if let MoviesTarget::Lan(target) = target {
                let mut capture = lan::LanCapture::new(target);
                capture.configure_live(live_lan);
                Some(capture)
            } else {
                None
            },
            wol: if let MoviesTarget::Wol(target) = target {
                Some(wol::WolCapture::new(target))
            } else {
                None
            },
            phase: Phase::MainMenu,
            route: Vec::new(),
            observed_campaign_startup: None,
            observed_campaign_runtime: None,
            observed_exit_cascade: false,
            abort_input: None,
            restart: None,
            restart_save_directory: output.with_extension(
                if matches!(
                    target,
                    MoviesTarget::CampaignStart {
                        target: CampaignStartTarget::Opening(_),
                        ..
                    }
                ) {
                    "opening-saves"
                } else {
                    "restart-saves"
                },
            ),
            saves_isolated: false,
            opening: match target {
                MoviesTarget::CampaignStart {
                    target: CampaignStartTarget::Opening(target),
                    ..
                } => Some(opening::OpeningCapture::new(
                    target,
                    output.with_extension("opening-saves"),
                )),
                _ => None,
            },
        }
    }

    pub(super) fn timeout(&self) -> Duration {
        if let Some(lan) = &self.lan {
            return lan.timeout();
        }
        if let Some(wol) = &self.wol {
            return wol.timeout();
        }
        if let MoviesTarget::CampaignStart {
            target: CampaignStartTarget::Restart(target),
            ..
        } = self.target
        {
            return target.timeout();
        }
        self.opening
            .as_ref()
            .map_or(Duration::from_secs(60), opening::OpeningCapture::timeout)
    }

    fn slide_settled(state: &AppState, kind: ShellSlideKind) -> bool {
        state.frontend.shell_first_paint_slide.is_none()
            && state.frontend.shell_slide_active_shell == Some(kind)
    }

    fn restore_neutral_pointer(state: &mut AppState) {
        state.match_state.input.cursor_x = EXPECTED_CURSOR_X as f32;
        state.match_state.input.cursor_y = EXPECTED_CURSOR_Y as f32;
    }

    pub(super) fn after_present(
        &mut self,
        state: &mut AppState,
        rendered: PresentedShell,
        frame: u32,
    ) -> Result<()> {
        if matches!(self.target, MoviesTarget::CampaignStart { .. }) && !self.saves_isolated {
            state
                .persistence
                .isolate_capture_saves(&self.restart_save_directory)?;
            self.saves_isolated = true;
        }
        if let Some(lan) = self.lan.as_mut() {
            return lan.after_present(state, rendered, frame);
        }
        if let Some(wol) = self.wol.as_mut() {
            return wol.after_present(state, rendered, frame);
        }
        if let Some(restart) = self.restart.as_mut() {
            return restart.after_present(state, rendered, &mut self.route, frame);
        }
        if matches!(
            self.target,
            MoviesTarget::CampaignStart {
                target: CampaignStartTarget::InitialSaved,
                ..
            }
        ) && self.phase == Phase::CampaignStarting
            && state.frontend.screen == GameScreen::InGame
        {
            self.observed_campaign_runtime = Some(campaign_runtime_receipt(state)?);
            let owner = crate::app::input::commands::preferred_local_owner_name(state)
                .context("initial save control has no local player")?;
            // The ordinary Options producer queues this before the admitted
            // frame. The automatic file must contain its consumed value.
            crate::app::input::commands::schedule_command(
                state,
                &owner,
                crate::sim::command::Command::SetGameSpeed { speed: 2 },
            );
            crate::app::match_runtime::sim_tick::advance_in_game_runtime_exact_step(state)?;
            self.route.push(
                json!({"frame": frame, "action": "initial campaign Logic step",
                "input": "ordinary Options SetGameSpeed producer", "speed": 2}),
            );
            let entries = state.persistence.repository.browser_entries();
            ensure!(entries.len() == 1, "initial save did not create one slot");
            let path = entries[0].0.path.clone();
            let bytes = state.persistence.repository.read(&path)?;
            let modified = std::fs::metadata(&path)?.modified()?;
            crate::app::persistence::commands::try_load_save_file(state, &path)
                .context("load the initial automatic save through the ordinary loader")?;
            crate::app::match_runtime::sim_tick::advance_in_game_runtime_exact_step(state)?;
            ensure!(
                state.persistence.repository.read(&path)? == bytes
                    && std::fs::metadata(&path)?.modified()? == modified,
                "loading the initial save rewrote it on the next Logic step"
            );
            self.route.push(
                json!({"frame": frame, "action": "initial save loaded and resumed",
                "save_file_unchanged": true}),
            );
            self.phase = Phase::CampaignInitialSaved;
            return Ok(());
        }
        if self.phase == Phase::CampaignInitialSaved {
            return Ok(());
        }
        let starting_campaign = matches!(self.target, MoviesTarget::CampaignStart { .. })
            && self.phase == Phase::CampaignStarting;
        let aborting_campaign = matches!(
            self.target,
            MoviesTarget::CampaignStart {
                target: CampaignStartTarget::AbortReturn,
                ..
            }
        ) && matches!(
            self.phase,
            Phase::CampaignAbortMenu | Phase::CampaignLeaving
        );
        ensure!(
            state.frontend.screen == GameScreen::MainMenu
                || ((starting_campaign || aborting_campaign)
                    && matches!(
                        state.frontend.screen,
                        GameScreen::Loading | GameScreen::InGame
                    )),
            "movies capture left its admitted shell/loading route"
        );
        ensure!(
            state.frontend.main_menu_shell_error.is_none(),
            "movies capture encountered the shell fallback"
        );
        if starting_campaign {
            if let Some(startup) = crate::app::loading::pump::loading_campaign_startup(state) {
                let observed = campaign_startup_receipt(startup);
                if let Some(prior) = self.observed_campaign_startup.as_ref() {
                    ensure!(
                        prior == &observed,
                        "selected campaign changed during loading"
                    );
                } else {
                    self.route
                        .push(json!({"frame": frame, "action": "campaign loading admitted"}));
                    self.observed_campaign_startup = Some(observed);
                }
            }
            if rendered == PresentedShell::FullscreenMovie {
                let displayed_frame = state
                    .frontend
                    .fullscreen_movie
                    .as_ref()
                    .context("campaign movie ended before its presented frame")?
                    .displayed_frame();
                // Play_Movie5BED40 accepts only the ordinary bare Escape release.
                // Present one actual frame before taking that handler.
                ensure!(
                    state.platform.live_modifiers.is_empty(),
                    "movie skip requires bare Escape"
                );
                App::fullscreen_movie_key(state, winit::keyboard::KeyCode::Escape, true);
                App::fullscreen_movie_key(state, winit::keyboard::KeyCode::Escape, false);
                self.route
                    .push(json!({"frame": frame, "action": "movie Escape release",
                    "movie_frame": displayed_frame}));
            }
            if matches!(
                self.target,
                MoviesTarget::CampaignStart {
                    target: CampaignStartTarget::AbortReturn,
                    ..
                }
            ) && state.frontend.screen == GameScreen::InGame
            {
                ensure!(
                    self.observed_campaign_startup.is_some(),
                    "installed campaign was not observed loading"
                );
                self.observed_campaign_runtime = Some(campaign_runtime_receipt(state)?);
                self.route
                    .push(json!({"frame": frame, "action": "first live observed", "tick": 0}));
                self.abort_input = Some(abort_return::AbortReturnInput::begin(
                    state,
                    &mut self.route,
                    frame,
                )?);
                self.phase = Phase::CampaignAbortMenu;
            }
            if let MoviesTarget::CampaignStart {
                target: CampaignStartTarget::Restart(target),
                ..
            } = self.target
                && state.frontend.screen == GameScreen::InGame
            {
                ensure!(
                    self.observed_campaign_startup.is_some(),
                    "installed campaign was not observed loading"
                );
                self.observed_campaign_runtime = Some(campaign_runtime_receipt(state)?);
                self.restart = Some(campaign_restart::RestartCapture::begin(
                    target,
                    state,
                    &mut self.route,
                    frame,
                    &self.restart_save_directory,
                )?);
                self.phase = Phase::CampaignRestart;
            }
            if state.frontend.screen == GameScreen::InGame
                && let Some(opening) = self.opening.as_mut()
            {
                ensure!(
                    self.observed_campaign_startup.is_some(),
                    "opening was not preceded by ordinary campaign loading"
                );
                opening.after_present(state)?;
            }
            return Ok(());
        }
        if aborting_campaign {
            return self.campaign_abort_after_present(state, frame);
        }
        if self.network_options_after_present(state, rendered, frame)? {
            return Ok(());
        }
        match (self.phase, rendered) {
            (Phase::MainMenu, PresentedShell::MainMenu) => {
                if steady_main_menu_capture_ready(MainMenuCaptureSnapshot::from_state(state))? {
                    let exit_phase = match self.target {
                        MoviesTarget::ExitConfirm => Some(Phase::Exit),
                        MoviesTarget::SlideOut {
                            kind: ShellSlideKind::MainMenu,
                            ..
                        } => Some(Phase::SlideOut),
                        _ => None,
                    };
                    if matches!(self.target, MoviesTarget::WolWelcome { .. }) {
                        self.route
                            .push(json!({"dialog": 0xe2, "frame": frame, "action": "Internet"}));
                        App::leave_shell_dialog(
                            state,
                            crate::app::frontend::shell_transition::ShellExitThen::MainMenu(
                                crate::ui::main_menu_shell::MainMenuShellAction::WwOnline,
                            ),
                        );
                        self.phase = Phase::Wol;
                        return Ok(());
                    }
                    if self.target == MoviesTarget::NetworkBounce {
                        self.route
                            .push(json!({"dialog": 0xe2, "frame": frame, "action": "Network"}));
                        App::leave_shell_dialog(
                            state,
                            crate::app::frontend::shell_transition::ShellExitThen::MainMenu(
                                crate::ui::main_menu_shell::MainMenuShellAction::Network,
                            ),
                        );
                        ensure!(
                            state.frontend.shell_exit.is_some(),
                            "Network did not start the 0xE2 teardown slide"
                        );
                        self.phase = Phase::NetworkLeaving;
                        return Ok(());
                    }
                    if matches!(
                        self.target,
                        MoviesTarget::Options0xD5 { .. } | MoviesTarget::NetworkOptions(_)
                    ) {
                        self.route
                            .push(json!({"dialog": 0xe2, "frame": frame, "action": "Options"}));
                        App::handle_main_menu_shell_action(
                            state,
                            crate::ui::main_menu_shell::MainMenuShellAction::Options,
                        );
                        self.phase = Phase::Options;
                        return Ok(());
                    }
                    if matches!(
                        self.target,
                        MoviesTarget::Campaign0x94 { .. }
                            | MoviesTarget::CampaignStart { .. }
                            | MoviesTarget::CampaignBack
                            | MoviesTarget::LoadSavedGame0xB7 { .. }
                    ) {
                        self.route.push(
                            json!({"dialog": 0xe2, "frame": frame, "action": "SinglePlayer"}),
                        );
                        App::handle_main_menu_shell_action(
                            state,
                            crate::ui::main_menu_shell::MainMenuShellAction::SinglePlayer,
                        );
                        self.phase = Phase::SinglePlayerPage;
                        return Ok(());
                    }
                    if let Some(next) = exit_phase {
                        self.route
                            .push(json!({"dialog": 0xe2, "frame": frame, "action": "ExitGame"}));
                        App::leave_shell_dialog(
                            state,
                            crate::app::frontend::shell_transition::ShellExitThen::MainMenu(
                                crate::ui::main_menu_shell::MainMenuShellAction::ExitGame,
                            ),
                        );
                        self.phase = next;
                        return Ok(());
                    }
                    self.route.push(
                        json!({"dialog": 0xe2, "frame": frame, "action": "MoviesAndCredits"}),
                    );
                    App::handle_main_menu_shell_action(
                        state,
                        crate::ui::main_menu_shell::MainMenuShellAction::MoviesAndCredits,
                    );
                    self.phase = Phase::Page;
                }
            }
            (Phase::Page, PresentedShell::MoviesAndCredits) => {
                if Self::slide_settled(state, ShellSlideKind::MoviesAndCredits) {
                    let action = match self.target {
                        MoviesTarget::Page0x101 => {
                            self.phase = Phase::Settling(SETTLE_FRAMES);
                            return Ok(());
                        }
                        MoviesTarget::List0x129
                        | MoviesTarget::List0x129Selected
                        | MoviesTarget::ListBackFirstFrame
                        | MoviesTarget::SlideOut {
                            kind: ShellSlideKind::MovieList,
                            ..
                        } => {
                            self.phase = Phase::List;
                            crate::ui::movies_credits_shell::MoviesCreditsAction::PlayMovies
                        }
                        MoviesTarget::FullList { .. } => {
                            state.persistence.options_profile.movie_progress =
                                crate::ui::movies_credits_shell::MovieProgress {
                                    soviet: 7,
                                    allied: 7,
                                };
                            self.route.push(json!({"frame": frame,
                                "action": "diagnostic movie progress override", "progress": [7, 7]}));
                            self.phase = Phase::List;
                            crate::ui::movies_credits_shell::MoviesCreditsAction::PlayMovies
                        }
                        MoviesTarget::Credits { .. } => {
                            self.phase = Phase::Credits;
                            crate::ui::movies_credits_shell::MoviesCreditsAction::ViewCredits
                        }
                        MoviesTarget::SneakPeek { .. } => {
                            self.phase = Phase::Movie;
                            crate::ui::movies_credits_shell::MoviesCreditsAction::SneakPeeks
                        }
                        MoviesTarget::ExitConfirm
                        | MoviesTarget::SlideOut { .. }
                        | MoviesTarget::Campaign0x94 { .. }
                        | MoviesTarget::CampaignStart { .. }
                        | MoviesTarget::CampaignBack
                        | MoviesTarget::LoadSavedGame0xB7 { .. }
                        | MoviesTarget::Options0xD5 { .. }
                        | MoviesTarget::NetworkOptions(_)
                        | MoviesTarget::Lan(_)
                        | MoviesTarget::Wol(_)
                        | MoviesTarget::NetworkBounce
                        | MoviesTarget::WolWelcome { .. } => {
                            bail!("{:?} capture reached the 0x101 page", self.target)
                        }
                    };
                    self.route.push(
                        json!({"dialog": 0x101, "frame": frame, "action": format!("{action:?}")}),
                    );
                    App::handle_movies_credits_action(state, action);
                }
            }
            (Phase::SinglePlayerPage, PresentedShell::SinglePlayer) => {
                if Self::slide_settled(state, ShellSlideKind::SinglePlayer) {
                    use crate::ui::single_player_shell::SinglePlayerShellAction;
                    let (action, phase) = match self.target {
                        MoviesTarget::LoadSavedGame0xB7 { .. } => {
                            ensure!(
                                state
                                    .frontend
                                    .single_player_shell_state
                                    .load_saved_game_enabled,
                                "Load Saved Game is disabled: no readable save in the saves directory"
                            );
                            (SinglePlayerShellAction::LoadSavedGame, Phase::LoadSavedGame)
                        }
                        _ => (SinglePlayerShellAction::NewCampaign, Phase::Campaign),
                    };
                    self.route.push(
                        json!({"dialog": 0x100, "frame": frame, "action": format!("{action:?}")}),
                    );
                    // The page button through the production teardown.
                    App::leave_shell_dialog(
                        state,
                        crate::app::frontend::shell_transition::ShellExitThen::SinglePlayer(action),
                    );
                    self.phase = phase;
                }
            }
            // Entry-slide frames present through the generic slide renderer.
            (Phase::Campaign, PresentedShell::Other | PresentedShell::Campaign)
                if matches!(
                    self.target,
                    MoviesTarget::Campaign0x94 {
                        entry_tick: Some(_),
                        ..
                    }
                ) =>
            {
                let MoviesTarget::Campaign0x94 {
                    entry_tick: Some(target),
                    ..
                } = self.target
                else {
                    unreachable!("matched above");
                };
                let tick = state
                    .frontend
                    .shell_first_paint_slide
                    .as_ref()
                    .filter(|_| {
                        state.frontend.shell_slide_active_shell == Some(ShellSlideKind::Campaign)
                    })
                    .and_then(|wave| wave.compatibility_tick());
                if let Some(tick) = tick {
                    ensure!(tick <= target, "entry slide passed tick {target}");
                    if tick == target {
                        if let Some(wave) = state.frontend.shell_first_paint_slide.as_mut() {
                            wave.hold_for_capture();
                        }
                        self.route.push(json!({"dialog": 0x94, "frame": frame,
                            "action": "hold entry slide", "tick": tick}));
                        self.phase = Phase::Settling(SETTLE_FRAMES);
                    }
                }
            }
            (Phase::LoadSavedGame, PresentedShell::Other | PresentedShell::LoadSavedGame) => {
                let MoviesTarget::LoadSavedGame0xB7 { entry_tick } = self.target else {
                    bail!("load phase without a load target");
                };
                if let Some(target) = entry_tick {
                    let tick = state
                        .frontend
                        .shell_first_paint_slide
                        .as_ref()
                        .filter(|_| {
                            state.frontend.shell_slide_active_shell
                                == Some(ShellSlideKind::LoadSavedGame)
                        })
                        .and_then(|wave| wave.compatibility_tick());
                    if let Some(tick) = tick {
                        ensure!(tick <= target, "entry slide passed tick {target}");
                        if tick == target {
                            if let Some(wave) = state.frontend.shell_first_paint_slide.as_mut() {
                                wave.hold_for_capture();
                            }
                            self.route.push(json!({"dialog": 0xb7, "frame": frame,
                                "action": "hold entry slide", "tick": tick}));
                            self.phase = Phase::Settling(SETTLE_FRAMES);
                        }
                    }
                } else if rendered == PresentedShell::LoadSavedGame
                    && Self::slide_settled(state, ShellSlideKind::LoadSavedGame)
                {
                    Self::restore_neutral_pointer(state);
                    App::handle_load_saved_game_mouse_move(state);
                    self.route.push(
                        json!({"dialog": 0xb7, "frame": frame, "action": "pointer at neutral"}),
                    );
                    self.phase = Phase::Settling(SETTLE_FRAMES);
                }
            }
            (Phase::Options, PresentedShell::Other | PresentedShell::Options) => {
                use crate::ui::main_menu_dialogs::options::{LauncherButton, OptionsPage};
                let MoviesTarget::Options0xD5 {
                    entry_tick,
                    hover,
                    page,
                } = self.target
                else {
                    bail!("options phase without an options target");
                };
                let showing = state
                    .frontend
                    .options_dialog
                    .as_ref()
                    .map(|dialog| dialog.page());
                if let Some(target) = entry_tick {
                    let tick = state
                        .frontend
                        .shell_first_paint_slide
                        .as_ref()
                        .filter(|_| {
                            state.frontend.shell_slide_active_shell == Some(ShellSlideKind::Options)
                        })
                        .and_then(|wave| wave.compatibility_tick());
                    if let Some(tick) = tick {
                        ensure!(tick <= target, "entry slide passed tick {target}");
                        if tick == target {
                            if let Some(wave) = state.frontend.shell_first_paint_slide.as_mut() {
                                wave.hold_for_capture();
                            }
                            self.route.push(json!({"dialog": 0xd5, "frame": frame,
                                "action": "hold entry slide", "tick": tick}));
                            self.phase = Phase::Settling(SETTLE_FRAMES);
                        }
                    }
                } else if rendered == PresentedShell::Options
                    && page == OptionsPage::Vera
                    && showing == Some(OptionsPage::Main)
                    && state.frontend.shell_exit.is_none()
                    && Self::slide_settled(state, ShellSlideKind::Options)
                {
                    let layout = crate::app::frontend::shell_layout::launcher_options_layout(
                        state,
                        OptionsPage::Main,
                    );
                    let rect = layout
                        .buttons
                        .iter()
                        .find(|(button, _)| *button == LauncherButton::Page(OptionsPage::Vera))
                        .map(|(_, rect)| *rect)
                        .context("D5 Vera Options button")?;
                    state.match_state.input.cursor_x = (rect.x + rect.w / 2) as f32;
                    state.match_state.input.cursor_y = (rect.y + rect.h / 2) as f32;
                    App::handle_launcher_options_mouse(state, None);
                    App::handle_launcher_options_mouse(state, Some(true));
                    App::handle_launcher_options_mouse(state, Some(false));
                    ensure!(
                        state.frontend.shell_exit.is_some(),
                        "Vera Options must run the main page's teardown slide"
                    );
                    self.route.push(json!({"dialog": 0xd5, "frame": frame,
                        "action": "Vera Options through ordinary pointer handler"}));
                } else if rendered == PresentedShell::Options
                    && showing == Some(page)
                    && state.frontend.shell_exit.is_none()
                    && Self::slide_settled(state, ShellSlideKind::options_page(page))
                {
                    Self::restore_neutral_pointer(state);
                    if let Some((x, y)) = hover {
                        state.match_state.input.cursor_x = x as f32;
                        state.match_state.input.cursor_y = y as f32;
                    }
                    App::handle_launcher_options_mouse(state, None);
                    self.route.push(json!({"dialog": 0xd5, "frame": frame,
                        "action": "pointer rests", "point": [
                            state.match_state.input.cursor_x, state.match_state.input.cursor_y]}));
                    self.phase = Phase::Settling(SETTLE_FRAMES);
                }
            }
            (Phase::Campaign, PresentedShell::Campaign) => {
                if Self::slide_settled(state, ShellSlideKind::Campaign) {
                    if let MoviesTarget::CampaignStart { side, target } = self.target
                        && let Some(scenario) = target.explicit_scenario()
                    {
                        let side = match side {
                            crate::ui::campaign_shell::CampaignSide::Allied => 0,
                            crate::ui::campaign_shell::CampaignSide::Soviet => 1,
                        };
                        let request = crate::app::frontend::cncnet_spawn::SpawnRequest::parse(
                            &format!(
                                "[Settings]\nIsSinglePlayer=Yes\nScenario={scenario}\nSide={side}\nDifficultyModeHuman=1\nDifficultyModeComputer=1\nGameSpeed=2\nSeed=1\n"
                            ),
                        )?;
                        App::start_spawn_session(state, request)?;
                        self.route.push(json!({"frame": frame,
                            "action": "explicit campaign spawn", "scenario": scenario,
                            "difficulty": 1, "seed": 1}));
                        self.phase = Phase::CampaignStarting;
                        return Ok(());
                    }
                    let layout = crate::ui::campaign_shell::compute_layout(
                        state.renderer.gpu.config.width,
                        state.renderer.gpu.config.height,
                    );
                    if let MoviesTarget::CampaignStart { side, .. } = self.target {
                        let rect = layout.emblem(side);
                        let point = (rect.x + rect.w / 2, rect.y + rect.h / 2);
                        state.match_state.input.cursor_x = point.0 as f32;
                        state.match_state.input.cursor_y = point.1 as f32;
                        App::handle_campaign_mouse_move(state);
                        App::handle_campaign_mouse_down(state);
                        let campaign = state
                            .frontend
                            .campaign
                            .as_ref()
                            .context("campaign page missing after press")?;
                        ensure!(
                            campaign.pressed() == Some(side),
                            "emblem press was not admitted"
                        );
                        self.route.push(json!({"dialog": 0x94, "frame": frame,
                            "action": "emblem press", "emblem": side.emblem(),
                            "campaign": side.campaign_name(), "point": [point.0, point.1],
                            "difficulty": campaign.difficulty()}));
                        self.phase = Phase::CampaignPressed;
                        return Ok(());
                    }
                    if self.target == MoviesTarget::CampaignBack {
                        let rect = layout
                            .page
                            .button_rect(crate::ui::campaign_shell::BACK_BUTTON)
                            .context("campaign Back geometry missing")?;
                        let point = (rect.x + rect.w / 2, rect.y + rect.h / 2);
                        state.match_state.input.cursor_x = point.0 as f32;
                        state.match_state.input.cursor_y = point.1 as f32;
                        App::handle_campaign_mouse_move(state);
                        App::handle_campaign_mouse_down(state);
                        App::handle_campaign_mouse_up(state);
                        ensure!(
                            state.frontend.shell_exit.is_some(),
                            "campaign Back did not start teardown"
                        );
                        self.route
                            .push(json!({"dialog": 0x94, "frame": frame, "action": "Back",
                            "point": [point.0, point.1]}));
                        Self::restore_neutral_pointer(state);
                        self.phase = Phase::CampaignReturning;
                        return Ok(());
                    }
                    if let MoviesTarget::Campaign0x94 {
                        press: Some(point),
                        hold_over,
                        ..
                    } = self.target
                    {
                        state.match_state.input.cursor_x = point.0 as f32;
                        state.match_state.input.cursor_y = point.1 as f32;
                        if let Some(over) = hold_over {
                            // The pointer reaches the slider, presses, and keeps
                            // the button down on its way to `over`.
                            App::handle_campaign_mouse_move(state);
                            App::handle_campaign_mouse_down(state);
                            state.match_state.input.cursor_x = over.0 as f32;
                            state.match_state.input.cursor_y = over.1 as f32;
                            App::handle_campaign_mouse_move(state);
                            self.route.push(json!({"dialog": 0x94, "frame": frame,
                                "action": "hold slider", "point": [point.0, point.1],
                                "over": [over.0, over.1]}));
                            self.phase = Phase::Settling(SETTLE_FRAMES);
                            return Ok(());
                        }
                        // The native helper clicks, then recenters the pointer.
                        App::handle_campaign_mouse_down(state);
                        App::handle_campaign_mouse_up(state);
                        self.route.push(json!({"dialog": 0x94, "frame": frame,
                            "action": "press slider", "point": [point.0, point.1]}));
                    }
                    Self::restore_neutral_pointer(state);
                    App::handle_campaign_mouse_move(state);
                    self.route.push(
                        json!({"dialog": 0x94, "frame": frame, "action": "pointer at neutral"}),
                    );
                    self.phase = Phase::Settling(SETTLE_FRAMES);
                }
            }
            (Phase::CampaignPressed, PresentedShell::Campaign) => {
                let MoviesTarget::CampaignStart { side, .. } = self.target else {
                    bail!("emblem press without a campaign start target");
                };
                ensure!(
                    state
                        .frontend
                        .campaign
                        .as_ref()
                        .is_some_and(|page| page.pressed() == Some(side)),
                    "campaign emblem press changed before release"
                );
                App::handle_campaign_mouse_up(state);
                ensure!(
                    state.frontend.shell_exit.is_some(),
                    "emblem release did not start teardown"
                );
                self.route.push(
                    json!({"dialog": 0x94, "frame": frame, "action": "emblem release",
                    "emblem": side.emblem(), "campaign": side.campaign_name(),
                    "difficulty": state.persistence.options_profile.difficulty}),
                );
                Self::restore_neutral_pointer(state);
                self.phase = Phase::CampaignStarting;
            }
            (Phase::CampaignReturning, PresentedShell::SinglePlayer) => {
                if Self::slide_settled(state, ShellSlideKind::SinglePlayer) {
                    self.route
                        .push(json!({"dialog": 0x100, "frame": frame, "action": "Back returned"}));
                    self.phase = Phase::Settling(SETTLE_FRAMES);
                }
            }
            (Phase::List, PresentedShell::MovieList) => {
                if Self::slide_settled(state, ShellSlideKind::MovieList) {
                    if let MoviesTarget::FullList { down_presses } = self.target {
                        state.match_state.input.cursor_x = LIST_DOWN_ARROW_POINT.0;
                        state.match_state.input.cursor_y = LIST_DOWN_ARROW_POINT.1;
                        for _ in 0..down_presses {
                            App::handle_movie_list_mouse_down(state);
                            App::handle_movie_list_mouse_up(state);
                        }
                        self.route.push(json!({"dialog": 0x129, "frame": frame,
                            "action": "press down arrow", "count": down_presses,
                            "point": [LIST_DOWN_ARROW_POINT.0, LIST_DOWN_ARROW_POINT.1]}));
                    }
                    if self.target == MoviesTarget::List0x129Selected {
                        state.match_state.input.cursor_x = LIST_ROW0_POINT.0;
                        state.match_state.input.cursor_y = LIST_ROW0_POINT.1;
                        App::handle_movie_list_mouse_down(state);
                        App::handle_movie_list_mouse_up(state);
                        self.route.push(json!({"dialog": 0x129, "frame": frame,
                            "action": "press row", "point": [LIST_ROW0_POINT.0, LIST_ROW0_POINT.1]}));
                    }
                    // The native helper recenters the pointer after each click;
                    // hover over the list writes its status help.
                    Self::restore_neutral_pointer(state);
                    App::handle_movie_list_mouse_move(state);
                    self.route.push(
                        json!({"dialog": 0x129, "frame": frame, "action": "pointer at neutral"}),
                    );
                    if matches!(
                        self.target,
                        MoviesTarget::SlideOut { .. } | MoviesTarget::ListBackFirstFrame
                    ) {
                        // Back (0x686) through the production teardown.
                        App::leave_shell_dialog(
                            state,
                            crate::app::frontend::shell_transition::ShellExitThen::MovieListBack,
                        );
                        ensure!(
                            state.frontend.shell_exit.is_some(),
                            "movie list Back did not start its teardown slide"
                        );
                        self.route
                            .push(json!({"dialog": 0x129, "frame": frame, "action": "Back"}));
                        self.phase = Phase::SlideOut;
                        return Ok(());
                    }
                    self.phase = Phase::Settling(SETTLE_FRAMES);
                }
            }
            (Phase::SlideOut, PresentedShell::MainMenu | PresentedShell::MovieList) => {
                if self.target == MoviesTarget::ListBackFirstFrame {
                    // Every slide-out frame presents; `ready` takes the first
                    // frame after the teardown commits.
                    return Ok(());
                }
                let MoviesTarget::SlideOut { kind, tick: target } = self.target else {
                    bail!("slide-out phase without a slide-out target");
                };
                let tick = crate::app::frontend::shell_transition::shell_exit_wave(state, kind)
                    .context("teardown slide ended before capture")?
                    .compatibility_tick()
                    .context("teardown slide without a tick clock")?;
                ensure!(tick <= target, "teardown slide passed tick {target}");
                if tick == target {
                    crate::app::frontend::shell_transition::hold_shell_exit_for_capture(state);
                    self.route
                        .push(json!({"dialog": kind.dialog_id().0, "frame": frame,
                        "action": "hold teardown slide", "tick": tick}));
                    self.phase = Phase::Settling(SETTLE_FRAMES);
                }
            }
            (Phase::Wol, PresentedShell::Other | PresentedShell::WolWelcome) => {
                let MoviesTarget::WolWelcome {
                    entry_tick,
                    hover,
                    press,
                } = self.target
                else {
                    bail!("WOL phase without a WOL target");
                };
                if let Some(target) = entry_tick {
                    let tick = state
                        .frontend
                        .shell_first_paint_slide
                        .as_ref()
                        .filter(|_| {
                            state.frontend.shell_slide_active_shell
                                == Some(ShellSlideKind::WolWelcome)
                        })
                        .and_then(|wave| wave.compatibility_tick());
                    if let Some(tick) = tick {
                        ensure!(tick <= target, "entry slide passed tick {target}");
                        if tick == target {
                            if let Some(wave) = state.frontend.shell_first_paint_slide.as_mut() {
                                wave.hold_for_capture();
                            }
                            self.route.push(json!({"dialog": 0x10e, "frame": frame,
                                "action": "hold entry slide", "tick": tick}));
                            self.phase = Phase::Settling(SETTLE_FRAMES);
                        }
                    }
                } else if rendered == PresentedShell::WolWelcome
                    && Self::slide_settled(state, ShellSlideKind::WolWelcome)
                {
                    let point = match press {
                        WolPress::None => None,
                        WolPress::MyInformation => Some(WOL_MY_INFORMATION_POINT),
                        WolPress::MainMenu => Some(WOL_MAIN_MENU_POINT),
                    };
                    if let Some((x, y)) = point {
                        state.match_state.input.cursor_x = x as f32;
                        state.match_state.input.cursor_y = y as f32;
                        App::handle_wol_mouse_down(state);
                        App::handle_wol_mouse_up(state);
                        // The native helper clicks, then recenters the pointer.
                        Self::restore_neutral_pointer(state);
                        ensure!(
                            state.frontend.shell_exit.is_some(),
                            "the 0x10E button did not start its teardown slide"
                        );
                        self.route.push(json!({"dialog": 0x10e, "frame": frame,
                            "action": format!("press {press:?}"), "point": [x, y]}));
                        self.phase = Phase::WolLeaving;
                        return Ok(());
                    }
                    Self::restore_neutral_pointer(state);
                    if let Some((x, y)) = hover {
                        state.match_state.input.cursor_x = x as f32;
                        state.match_state.input.cursor_y = y as f32;
                    }
                    App::handle_wol_mouse_move(state);
                    self.route.push(json!({"dialog": 0x10e, "frame": frame,
                        "action": "pointer rests", "point": [
                            state.match_state.input.cursor_x, state.match_state.input.cursor_y]}));
                    self.phase = Phase::Settling(SETTLE_FRAMES);
                }
            }
            (
                Phase::WolLeaving,
                PresentedShell::Other | PresentedShell::WolWelcome | PresentedShell::MainMenu,
            ) => {
                let MoviesTarget::WolWelcome { press, .. } = self.target else {
                    bail!("WOL phase without a WOL target");
                };
                if state.frontend.shell_exit.is_some() {
                    return Ok(());
                }
                match press {
                    WolPress::MyInformation => {
                        ensure!(
                            wol_api_missing_shown(state),
                            "the WOL action did not show TXT_APIMISSING"
                        );
                        Self::restore_neutral_pointer(state);
                        App::handle_wol_mouse_move(state);
                        self.route.push(
                            json!({"dialog": 0xd0, "frame": frame, "action": "TXT_APIMISSING"}),
                        );
                        self.phase = Phase::Settling(SETTLE_FRAMES);
                    }
                    WolPress::MainMenu => {
                        self.route.push(
                            json!({"dialog": 0x10e, "frame": frame, "action": "teardown ended"}),
                        );
                        self.phase = Phase::NetworkReturning;
                    }
                    WolPress::None => bail!("WOL leave phase without a press"),
                }
            }
            (Phase::NetworkLeaving, PresentedShell::MainMenu | PresentedShell::Other) => {
                if state.frontend.shell_exit.is_none() {
                    self.route
                        .push(json!({"dialog": 0xe2, "frame": frame, "action": "teardown ended"}));
                    self.phase = Phase::NetworkReturning;
                }
            }
            (Phase::NetworkReturning, PresentedShell::MainMenu | PresentedShell::Other) => {
                if state.frontend.shell_slide_active_shell == Some(ShellSlideKind::MainMenu)
                    && state.frontend.shell_first_paint_slide.is_some()
                {
                    self.route.push(
                        json!({"dialog": 0xe2, "frame": frame, "action": "new 0xE2 entry slide"}),
                    );
                    self.phase = Phase::NetworkReturned;
                }
            }
            (Phase::NetworkReturned, PresentedShell::MainMenu) => {
                if steady_main_menu_capture_ready(MainMenuCaptureSnapshot::from_state(state))? {
                    self.route
                        .push(json!({"dialog": 0xe2, "frame": frame, "action": "settled"}));
                    self.phase = Phase::Settling(SETTLE_FRAMES);
                }
            }
            (Phase::Credits, PresentedShell::CreditsRoll) => {
                let MoviesTarget::Credits { frame: target } = self.target else {
                    bail!("credits phase without a credits target");
                };
                let session = state
                    .frontend
                    .credits_roll
                    .as_mut()
                    .context("credits roll ended before capture")?;
                session.roll.hold_at_frame_for_capture(target);
                ensure!(
                    session.roll.frame() == target,
                    "credits roll could not reach frame {target}"
                );
                // Redraw the held frame even if the capture window is inactive.
                session.last_drawn.clear();
                self.route.push(
                    json!({"presentation": "Show_Credits", "frame": frame, "roll_frame": target}),
                );
                self.phase = Phase::Settling(SETTLE_FRAMES);
            }
            (Phase::Movie, PresentedShell::FullscreenMovie) => {
                let MoviesTarget::SneakPeek { frame: target } = self.target else {
                    bail!("movie phase without a movie target");
                };
                let movie = state
                    .frontend
                    .fullscreen_movie
                    .as_mut()
                    .context("Play_Movie ended before capture")?;
                movie.hold_at_frame_for_capture(&state.renderer.gpu, target)?;
                ensure!(
                    movie.displayed_frame() == target,
                    "Play_Movie could not reach frame {target}"
                );
                self.route.push(
                    json!({"presentation": "Play_Movie", "frame": frame, "movie_frame": target}),
                );
                self.phase = Phase::Settling(SETTLE_FRAMES);
            }
            (Phase::Exit, PresentedShell::MainMenu) => {
                if state.frontend.shell_message_box.is_some() && state.frontend.shell_exit.is_none()
                {
                    self.route
                        .push(json!({"state": 6, "frame": frame, "action": "confirmation shown"}));
                    self.phase = Phase::Settling(SETTLE_FRAMES);
                }
            }
            (Phase::Settling(remaining), _) if remaining > 0 => {
                self.phase = Phase::Settling(remaining - 1);
            }
            _ => {}
        }
        Ok(())
    }

    fn campaign_abort_after_present(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        if self.phase == Phase::CampaignAbortMenu {
            if self
                .abort_input
                .as_mut()
                .context("abort input owner")?
                .after_present(state, &mut self.route, frame)?
            {
                self.phase = Phase::CampaignLeaving;
            }
            return Ok(());
        }
        match self.phase {
            Phase::CampaignLeaving => {
                if state.match_state.scenario_exit.is_some() && !self.observed_exit_cascade {
                    ensure!(
                        state.match_state.scenario_outcome.is_none(),
                        "EXIT entered a House outcome route"
                    );
                    let sim = &state
                        .match_state
                        .sim_runtime
                        .as_ref()
                        .context("EXIT lost its installed runtime")?
                        .simulation;
                    ensure!(
                        sim.quit_requested()
                            && !sim
                                .pending_command_snapshot()
                                .iter()
                                .any(|command| matches!(
                                    command.payload,
                                    crate::sim::command::Command::ExitMatch
                                )),
                        "exit cascade began before the queued EXIT was consumed"
                    );
                    self.route.push(
                        json!({"frame": frame, "action": "EXIT consumed", "tick": sim.session().tick(),
                        "scenario_exit_present": true, "scenario_outcome_present": false,
                        "quit_requested": sim.quit_requested()}),
                    );
                    self.observed_exit_cascade = true;
                }
                if state.frontend.screen == GameScreen::MainMenu {
                    ensure!(
                        self.observed_exit_cascade,
                        "campaign returned without observing the queued EXIT cascade"
                    );
                    Self::restore_neutral_pointer(state);
                    if steady_main_menu_capture_ready(MainMenuCaptureSnapshot::from_state(state))? {
                        let cleanup = campaign_abort_cleanup_receipt(state)?;
                        self.route.push(json!({"frame": frame, "action": "abort returned", "dialog": 0xe2,
                            "tick": cleanup["retained_tick"], "campaign_mission_counter": cleanup["campaign_mission_counter"]}));
                        self.phase = Phase::Settling(SETTLE_FRAMES);
                    }
                }
            }
            _ => bail!("campaign abort route entered an unrelated capture phase"),
        }
        Ok(())
    }

    pub(super) fn ready(&self, state: &AppState) -> Result<bool> {
        if let Some(lan) = self.lan.as_ref() {
            return lan.ready(state);
        }
        if let Some(wol) = self.wol.as_ref() {
            return wol.ready(state);
        }
        if matches!(
            self.target,
            MoviesTarget::CampaignStart {
                target: CampaignStartTarget::AbortReturn,
                ..
            }
        ) {
            if self.phase != Phase::Settling(0) {
                return Ok(false);
            }
            ensure!(
                self.observed_campaign_runtime.is_some() && self.observed_exit_cascade,
                "campaign abort return omitted its installed-game/EXIT observations"
            );
            campaign_abort_cleanup_receipt(state)?;
            return steady_main_menu_capture_ready(MainMenuCaptureSnapshot::from_state(state));
        }
        if let Some(restart) = self.restart.as_ref() {
            return restart.ready(state);
        }
        if let MoviesTarget::CampaignStart { side, target } = self.target {
            if target == CampaignStartTarget::InitialSaved
                && self.phase == Phase::CampaignInitialSaved
            {
                initial_save_receipt(state, &self.restart_save_directory)?;
                return Ok(true);
            }
            if self.phase != Phase::CampaignStarting {
                return Ok(false);
            }
            if target.explicit_scenario().is_none()
                && let Some(startup) = crate::app::loading::pump::loading_campaign_startup(state)
            {
                ensure!(
                    startup
                        .campaign()
                        .id()
                        .eq_ignore_ascii_case(side.campaign_name()),
                    "loading admitted a different campaign than the released emblem"
                );
            }
            return match target {
                CampaignStartTarget::LoadingFirst => {
                    let ready = state.frontend.screen == GameScreen::Loading
                        && crate::app::loading::pump::next_native_loading_frame(state)
                            == Some(crate::app::loading::pump::NextLoadingFrame::First);
                    if ready {
                        ensure!(
                            crate::app::loading::pump::loading_campaign_startup(state).is_some(),
                            "first loading frame has no campaign request"
                        );
                    }
                    Ok(ready)
                }
                CampaignStartTarget::FirstLive | CampaignStartTarget::ExplicitFirstLive(_) => {
                    if state.frontend.screen != GameScreen::InGame {
                        return Ok(false);
                    }
                    ensure!(
                        self.observed_campaign_startup.is_some(),
                        "first live frame was not preceded by ordinary campaign loading"
                    );
                    campaign_runtime_receipt(state)?;
                    Ok(true)
                }
                CampaignStartTarget::InitialSaved => Ok(false),
                CampaignStartTarget::Opening(_) => {
                    if state.frontend.screen != GameScreen::InGame {
                        return Ok(false);
                    }
                    self.opening
                        .as_ref()
                        .context("opening capture missing")?
                        .ready(state)
                }
                CampaignStartTarget::AbortReturn => unreachable!("handled above"),
                // The restart driver takes over at the first live frame.
                CampaignStartTarget::Restart(_) => Ok(false),
            };
        }
        if self.target == MoviesTarget::ListBackFirstFrame {
            // Checked after acquisition: once the teardown has committed, this
            // very frame must be the recreated page's entry tick 0.
            if self.phase != Phase::SlideOut || state.frontend.shell_exit.is_some() {
                return Ok(false);
            }
            let entry_tick = state
                .frontend
                .shell_first_paint_slide
                .as_ref()
                .and_then(|wave| wave.compatibility_tick());
            ensure!(
                state.frontend.shell_route.movies_and_credits() && entry_tick == Some(0),
                "the frame after the 0x129 teardown is not the 0x101 entry tick 0 \
                 (route M&C {}, entry tick {entry_tick:?})",
                state.frontend.shell_route.movies_and_credits()
            );
            return Ok(true);
        }
        if self.phase != Phase::Settling(0) {
            return Ok(false);
        }
        // Steady page frames wait for the heading's and the status line's
        // kind-1 reveals to finish.
        let heading_settled = match self.target {
            MoviesTarget::Page0x101
            | MoviesTarget::List0x129
            | MoviesTarget::List0x129Selected
            | MoviesTarget::FullList { .. }
            | MoviesTarget::CampaignBack
            | MoviesTarget::Campaign0x94 {
                entry_tick: None, ..
            } => {
                state.frontend.shell_page_title.is_terminal()
                    && state.frontend.shell_status_line.is_terminal()
            }
            MoviesTarget::Campaign0x94 {
                entry_tick: Some(_),
                ..
            }
            | MoviesTarget::LoadSavedGame0xB7 {
                entry_tick: Some(_),
            }
            | MoviesTarget::Options0xD5 {
                entry_tick: Some(_),
                ..
            } => true,
            MoviesTarget::LoadSavedGame0xB7 { entry_tick: None }
            | MoviesTarget::Options0xD5 {
                entry_tick: None, ..
            } => {
                state.frontend.shell_page_title.is_terminal()
                    && state.frontend.shell_status_line.is_terminal()
            }
            MoviesTarget::Credits { .. }
            | MoviesTarget::CampaignStart { .. }
            | MoviesTarget::SneakPeek { .. }
            | MoviesTarget::ExitConfirm
            | MoviesTarget::SlideOut { .. }
            | MoviesTarget::ListBackFirstFrame
            | MoviesTarget::NetworkBounce
            | MoviesTarget::WolWelcome {
                entry_tick: Some(_),
                ..
            }
            | MoviesTarget::WolWelcome {
                press: WolPress::MyInformation | WolPress::MainMenu,
                ..
            } => true,
            MoviesTarget::Lan(_) => unreachable!("LAN driver handled above"),
            MoviesTarget::Wol(_) => unreachable!("WOL driver handled above"),
            MoviesTarget::NetworkOptions(target) => {
                target.is_message()
                    || target == NetworkOptionsTarget::SaveReturn
                    || (state.frontend.shell_page_title.is_terminal()
                        && state.frontend.shell_status_line.is_terminal())
            }
            MoviesTarget::WolWelcome { .. } => {
                state.frontend.shell_page_title.is_terminal()
                    && state.frontend.shell_status_line.is_terminal()
            }
        };
        if !heading_settled {
            return Ok(false);
        }
        let expected = match self.target {
            MoviesTarget::Page0x101 => state.frontend.shell_route.movies_and_credits(),
            MoviesTarget::List0x129
            | MoviesTarget::List0x129Selected
            | MoviesTarget::FullList { .. } => {
                state.frontend.shell_route.movie_list() && state.frontend.movie_list.is_some()
            }
            MoviesTarget::Credits { .. } => state.frontend.credits_roll.is_some(),
            MoviesTarget::SneakPeek { .. } => state.frontend.fullscreen_movie.is_some(),
            MoviesTarget::ExitConfirm => state.frontend.shell_message_box.is_some(),
            MoviesTarget::NetworkBounce
            | MoviesTarget::WolWelcome {
                press: WolPress::MainMenu,
                ..
            } => {
                state.frontend.shell_exit.is_none()
                    && crate::app::frontend::shell_transition::current_shell_slide_target(state)
                        == Some(ShellSlideKind::MainMenu)
            }
            MoviesTarget::WolWelcome {
                press: WolPress::MyInformation,
                ..
            } => wol_api_missing_shown(state),
            MoviesTarget::WolWelcome { entry_tick, .. } => {
                state.frontend.shell_route.wol()
                    && entry_tick.is_none_or(|target| {
                        state
                            .frontend
                            .shell_first_paint_slide
                            .as_ref()
                            .and_then(|wave| wave.compatibility_tick())
                            == Some(target)
                    })
            }
            MoviesTarget::SlideOut { kind, tick } => {
                crate::app::frontend::shell_transition::shell_exit_wave(state, kind)
                    .and_then(|wave| wave.compatibility_tick())
                    == Some(tick)
            }
            MoviesTarget::Campaign0x94 { entry_tick, .. } => {
                state.frontend.shell_route.campaign()
                    && state.frontend.campaign.is_some()
                    && entry_tick.is_none_or(|target| {
                        state
                            .frontend
                            .shell_first_paint_slide
                            .as_ref()
                            .and_then(|wave| wave.compatibility_tick())
                            == Some(target)
                    })
            }
            MoviesTarget::LoadSavedGame0xB7 { entry_tick } => {
                state.frontend.shell_route.load_saved_game()
                    && state.frontend.load_saved_game.is_some()
                    && entry_tick.is_none_or(|target| {
                        state
                            .frontend
                            .shell_first_paint_slide
                            .as_ref()
                            .and_then(|wave| wave.compatibility_tick())
                            == Some(target)
                    })
            }
            MoviesTarget::Lan(_) => unreachable!("LAN driver handled above"),
            MoviesTarget::Wol(_) => unreachable!("WOL driver handled above"),
            MoviesTarget::NetworkOptions(target) => self.network_options_ready(state, target)?,
            MoviesTarget::Options0xD5 {
                entry_tick, page, ..
            } => {
                state
                    .frontend
                    .options_dialog
                    .as_ref()
                    .is_some_and(|dialog| dialog.page() == page)
                    && entry_tick.is_none_or(|target| {
                        state
                            .frontend
                            .shell_first_paint_slide
                            .as_ref()
                            .and_then(|wave| wave.compatibility_tick())
                            == Some(target)
                    })
            }
            MoviesTarget::ListBackFirstFrame => unreachable!("handled above"),
            MoviesTarget::CampaignStart { .. } => unreachable!("handled above"),
            MoviesTarget::CampaignBack => {
                state.frontend.screen == GameScreen::MainMenu
                    && state.frontend.shell_route.single_player()
                    && state.frontend.campaign.is_none()
                    && state.frontend.loading_session.is_none()
                    && state.frontend.shell_exit.is_none()
                    && Self::slide_settled(state, ShellSlideKind::SinglePlayer)
            }
        };
        ensure!(expected, "movies capture route changed before readback");
        Ok(true)
    }

    pub(super) fn manifest(
        &self,
        request: &ShellCaptureRequest,
        format: wgpu::TextureFormat,
        pixels: &[u8],
        frame: u32,
        state: &AppState,
    ) -> Result<Value> {
        let mut manifest = json!({
            "schema_version": "vera20k.movies-shell-capture.v1",
            "checkpoint": request.checkpoint.as_str(),
            "parity_certification": "NONE", "presenter_domain": "final-swapchain-after-rgb565",
            "surface": {"width": request.width, "height": request.height, "format": format!("{format:?}"),
                "pixel_layout": "BGRA8", "row_order": "top-left", "row_stride": request.width * 4},
            "cursor": {"x": request.cursor_x, "y": request.cursor_y, "policy": "software-composited"},
            "route": self.route, "capture_frame": frame,
            "frame": {"path": FRAME_FILE_NAME, "byte_length": pixels.len(),
                "sha256": crate::util::sha256::sha256_hex(pixels)}
        });
        if let MoviesTarget::CampaignStart { target, .. } = self.target {
            let startup = crate::app::loading::pump::loading_campaign_startup(state)
                .map(campaign_startup_receipt)
                .or_else(|| self.observed_campaign_startup.clone())
                .context("campaign startup receipt missing at readback")?;
            let map_source = match target {
                CampaignStartTarget::LoadingFirst => {
                    crate::app::loading::pump::loading_map_source(state)
                }
                CampaignStartTarget::FirstLive
                | CampaignStartTarget::InitialSaved
                | CampaignStartTarget::ExplicitFirstLive(_)
                | CampaignStartTarget::AbortReturn
                | CampaignStartTarget::Opening(_)
                | CampaignStartTarget::Restart(_) => state.match_state.loaded_map_source.as_ref(),
            }
            .context("campaign selected map source missing at readback")?;
            // Readiness is queried before the first loading render prepares
            // the map. At readback the ordinary renderer has completed that
            // preparation, so every startup receipt requires its retail bytes.
            ensure!(
                matches!(
                    map_source,
                    crate::map::source::LoadedMapSource::Loose { .. }
                        | crate::map::source::LoadedMapSource::Mix { .. }
                ),
                "campaign capture must retain the selected retail map bytes"
            );
            manifest["schema_version"] = json!("vera20k.campaign-start-capture.v1");
            manifest["startup"] = startup;
            manifest["map_admission"] = json!("prepared");
            manifest["selected_map_source"] = serde_json::to_value(map_source)?;
            manifest["process_source_ini_hashes"] = process_source_ini_hashes(state)?;
            manifest["checkpoint_phase"] = json!(match target {
                CampaignStartTarget::LoadingFirst => "loading-first-frame",
                CampaignStartTarget::FirstLive => "first-live-frame",
                CampaignStartTarget::InitialSaved => "initial-save",
                CampaignStartTarget::ExplicitFirstLive(_) => "explicit-first-live-frame",
                CampaignStartTarget::AbortReturn => "abort-return",
                CampaignStartTarget::Opening(target) => target.phase(),
                CampaignStartTarget::Restart(RestartTarget::AbortShown) => "abort-restart-shown",
                CampaignStartTarget::Restart(RestartTarget::AbortRestarted) => "abort-restarted",
                CampaignStartTarget::Restart(RestartTarget::DefeatPrompt) => "defeat-replay-prompt",
                CampaignStartTarget::Restart(RestartTarget::DefeatReplayed) => "defeat-replayed",
                CampaignStartTarget::Restart(RestartTarget::DefeatReturned) => "defeat-returned",
                CampaignStartTarget::Restart(RestartTarget::DefeatReturnedEnter) =>
                    "defeat-returned-enter",
                CampaignStartTarget::Restart(RestartTarget::DefeatReturnedNumpadEnter) =>
                    "defeat-returned-numpad-enter",
                CampaignStartTarget::Restart(RestartTarget::DefeatReturnedEscape) =>
                    "defeat-returned-escape",
                CampaignStartTarget::Restart(RestartTarget::DefeatReturnedRepeatedEnter) =>
                    "defeat-returned-repeated-enter",
                CampaignStartTarget::Restart(RestartTarget::Restate) => "restate",
                CampaignStartTarget::Restart(RestartTarget::ObjectiveExpiry) =>
                    "objective-expired-after-menu",
                CampaignStartTarget::Restart(RestartTarget::SavedVictory) =>
                    "saved-victory-next-mission",
                CampaignStartTarget::Restart(RestartTarget::AlternateHomeVictory) =>
                    "alternate-home-victory-next-mission",
                CampaignStartTarget::Restart(RestartTarget::InheritedTimerVictory) =>
                    "inherited-timer-victory-next-mission",
                CampaignStartTarget::Restart(RestartTarget::MapSelectFailurePrompt) =>
                    "mapsel-failure-prompt",
                CampaignStartTarget::Restart(RestartTarget::MapSelectFailureReplayed) =>
                    "mapsel-failure-replayed",
                CampaignStartTarget::Restart(RestartTarget::MapSelectFailureEscape) =>
                    "mapsel-failure-escape",
                CampaignStartTarget::Restart(RestartTarget::SavedAbortRestarted) =>
                    "saved-abort-restarted",
                CampaignStartTarget::Restart(RestartTarget::MenuDefeatReplayed) =>
                    "menu-defeat-replayed",
            });
            manifest["runtime"] = match target {
                CampaignStartTarget::LoadingFirst => Value::Null,
                CampaignStartTarget::FirstLive | CampaignStartTarget::ExplicitFirstLive(_) => {
                    campaign_runtime_receipt(state)?
                }
                CampaignStartTarget::InitialSaved => installed_campaign_runtime_receipt(state)?,
                CampaignStartTarget::AbortReturn | CampaignStartTarget::Restart(_) => self
                    .observed_campaign_runtime
                    .clone()
                    .context("campaign route has no first-live runtime observation")?,
                CampaignStartTarget::Opening(_) => installed_campaign_runtime_receipt(state)?,
            };
            if matches!(target, CampaignStartTarget::Restart(target) if target.next_scenario().is_some())
            {
                // This readback presents the next mission. The restart receipt
                // separately retains the initial and loaded mission identities.
                manifest["startup"] = campaign_startup_receipt(
                    state
                        .match_state
                        .startup
                        .installed_campaign()
                        .context("continued mission has no campaign admission")?,
                );
                manifest["runtime"] = campaign_runtime_receipt(state)?;
            }
            if target == CampaignStartTarget::AbortReturn {
                manifest["runtime_observation"] = json!("first-live-frame-before-Escape-press");
                manifest["cleanup"] = campaign_abort_cleanup_receipt(state)?;
            }
            if target == CampaignStartTarget::InitialSaved {
                manifest["initial_save"] =
                    initial_save_receipt(state, &self.restart_save_directory)?;
            }
            manifest["coverage"] = json!({
                "loading_readback": "first presented loading screen only; later synchronous milestones are not captured",
                "first_live": "first InGame draw before any simulation tick",
                "mission_opening_1308": "unresolved; this receipt does not certify scripted mission opening",
            });
            if let Some(opening) = &self.opening {
                manifest["opening"] = opening.receipt(state)?;
                manifest["coverage"]["first_live"] =
                    json!("tick-zero installation observed before exact production steps");
                manifest["coverage"]["mission_opening_1308"] = json!(
                    "selected SOV01 checkpoint after ordinary loading; app/render integration only, not a native whole-scene parity certificate"
                );
                manifest["coverage"]["audio"] = json!(
                    "ordinary audio owners serviced; hardware audio timing and waveform are not captured"
                );
            }
            if target == CampaignStartTarget::AbortReturn {
                manifest["coverage"]["abort_audio"] =
                    json!("ordinary exit cascade observed; device audio output is not captured");
                manifest["coverage"]["campaign_b6_restart"] =
                    json!("selected Leave path only; Restart has its own targets");
            }
            if let Some(restart) = &self.restart {
                manifest["restart"] = restart.receipt();
            }
        } else if self.target == MoviesTarget::CampaignBack {
            manifest["schema_version"] = json!("vera20k.campaign-start-capture.v1");
            manifest["checkpoint_phase"] = json!("campaign-back-return");
            manifest["shell_return"] = json!({"screen": format!("{:?}", state.frontend.screen),
                "route": format!("{:?}", state.frontend.shell_route),
                "campaign_page_present": state.frontend.campaign.is_some(),
                "loading_session_present": state.frontend.loading_session.is_some()});
        }
        if let MoviesTarget::NetworkOptions(target) = self.target {
            manifest["network_options"] = self.network_options_receipt(state, target)?;
        }
        if let Some(lan) = self.lan.as_ref() {
            manifest["lan"] = lan.receipt(state)?;
        }
        if let Some(wol) = self.wol.as_ref() {
            manifest["wol"] = wol.receipt(state);
        }
        Ok(manifest)
    }

    pub(super) fn cleanup(&mut self) -> Result<()> {
        if let Some(lan) = self.lan.as_mut() {
            lan.cleanup()?;
        }
        Ok(())
    }

    pub(super) fn owns_simulation_steps(&self, state: &AppState) -> bool {
        if let Some(lan) = self.lan.as_ref() {
            return lan.owns_simulation_steps(state);
        }
        if let Some(wol) = self.wol.as_ref() {
            return wol.owns_simulation_steps(state);
        }
        if let Some(restart) = self.restart.as_ref() {
            return restart.owns_simulation_steps(state);
        }
        if matches!(
            self.target,
            MoviesTarget::CampaignStart {
                target: CampaignStartTarget::Opening(opening::OpeningTarget::RealtimeUnlocked),
                ..
            }
        ) {
            return false;
        }

        matches!(
            self.target,
            MoviesTarget::CampaignStart {
                target: CampaignStartTarget::FirstLive
                    | CampaignStartTarget::InitialSaved
                    | CampaignStartTarget::ExplicitFirstLive(_)
                    | CampaignStartTarget::AbortReturn
                    | CampaignStartTarget::Opening(_)
                    | CampaignStartTarget::Restart(_),
                ..
            }
        ) && matches!(
            self.phase,
            Phase::CampaignStarting | Phase::CampaignInitialSaved
        ) && state.frontend.screen == GameScreen::InGame
    }
}

/// The visit shows `0xD0` with `TXT_APIMISSING` over the backdrop.
fn wol_api_missing_shown(state: &AppState) -> bool {
    let expected = App::csf_label(state, "TXT_APIMISSING", "TXT_APIMISSING");
    state
        .frontend
        .wol
        .as_ref()
        .and_then(|visit| visit.message())
        .is_some_and(|message| message.body == expected)
}

/// Same physical-window to retained-render coordinates as CursorMoved in the
/// window handler. The existing hit owner maps them back to its loaded pixels.
fn set_window_pointer_to_rect_center(
    state: &mut AppState,
    rect: crate::ui::shell::geom::RectPx,
) -> [i32; 2] {
    let point = [rect.x + rect.w / 2, rect.y + rect.h / 2];
    state.match_state.input.cursor_x =
        point[0] as f32 * state.render_width() as f32 / state.renderer.gpu.config.width as f32;
    state.match_state.input.cursor_y =
        point[1] as f32 * state.render_height() as f32 / state.renderer.gpu.config.height as f32;
    point
}

fn campaign_startup_receipt(
    startup: &crate::app::match_bootstrap::PreparedCampaignStartup,
) -> Value {
    let campaign = startup.campaign();
    json!({
        "campaign": {"id": campaign.id(), "index": startup.campaign_index(), "cd": campaign.cd(),
            "scenario": campaign.scenario(), "final_movie": campaign.final_movie()},
        "scenario": startup.scenario(),
        "difficulty": startup.difficulty(),
        "seed": {"value": startup.seed.value, "source": format!("{:?}", startup.seed.source),
            "seed_authority_certifying": startup.seed.seed_authority_certifying},
    })
}

fn process_source_ini_hashes(state: &AppState) -> Result<Value> {
    let owner = state
        .process_assets
        .native_rules()
        .context("campaign has no process Rules owner")?;
    Ok(
        json!({"domain": "IniFile.content_hash parsed cache; not retail byte SHA256",
        "sources": owner.selected_source_ini_hashes().map(|(name, hash)| json!({
            "name": name, "parsed_cache_hash": hash.map(|value| format!("{value:016x}")),
        }))}),
    )
}

fn campaign_abort_cleanup_receipt(state: &AppState) -> Result<Value> {
    ensure!(
        state.frontend.screen == GameScreen::MainMenu
            && state.frontend.shell_route == crate::app::shell_route::ShellRoute::MainMenu,
        "campaign abort did not return to the ordinary main menu"
    );
    ensure!(
        state.frontend.loading_session.is_none()
            && crate::app::loading::pump::loading_campaign_startup(state).is_none()
            && state.match_state.startup.startup().is_none()
            && state.match_state.startup.receipt().is_none()
            && !state.match_state.startup.campaign_installed()
            && !state.process_assets.is_leased()
            && state.process_assets.is_available(),
        "campaign abort retained an active startup or loading asset lease"
    );
    ensure!(
        state.frontend.campaign.is_none()
            && state.frontend.fullscreen_movie.is_none()
            && state.match_state.scenario_exit.is_none()
            && state.match_state.scenario_outcome.is_none()
            && state.match_state.match_presentation.in_game_menu
                == crate::ui::pause_menu::InGameMenuState::Closed,
        "campaign abort retained a modal/movie/outcome transition"
    );
    let sim = &state
        .match_state
        .sim_runtime
        .as_ref()
        .context("ordinary return unexpectedly discarded the retained campaign runtime")?
        .simulation;
    let mission_counter = sim
        .session()
        .campaign_mission_counter()
        .context("retained runtime lost campaign state")?;
    let pending_exit_commands = sim
        .pending_command_snapshot()
        .iter()
        .filter(|command| matches!(command.payload, crate::sim::command::Command::ExitMatch))
        .count();
    ensure!(
        mission_counter == 1
            && sim.quit_requested()
            && pending_exit_commands == 0
            && !sim.session().started(),
        "campaign return did not consume EXIT/reset the retained Session counter/retire started"
    );
    Ok(json!({
        "screen": format!("{:?}", state.frontend.screen), "route": format!("{:?}", state.frontend.shell_route),
        "loading_session_present": state.frontend.loading_session.is_some(),
        "loading_campaign_startup_present": crate::app::loading::pump::loading_campaign_startup(state).is_some(),
        "accepted_match_startup_present": state.match_state.startup.startup().is_some(),
        "match_startup_receipt_present": state.match_state.startup.receipt().is_some(),
        "campaign_installation_present": state.match_state.startup.campaign_installed(),
        "asset_manager_leased": state.process_assets.is_leased(), "asset_manager_available": state.process_assets.is_available(),
        "campaign_page_present": state.frontend.campaign.is_some(), "fullscreen_movie_present": state.frontend.fullscreen_movie.is_some(),
        "scenario_exit_present": state.match_state.scenario_exit.is_some(), "scenario_outcome_present": state.match_state.scenario_outcome.is_some(),
        "in_game_menu": format!("{:?}", state.match_state.match_presentation.in_game_menu),
        "retained_runtime_present": state.match_state.sim_runtime.is_some(), "retained_tick": sim.session().tick(),
        "retained_quit_requested": sim.quit_requested(),
        "retained_scenario_started": sim.session().started(),
        "campaign_mission_counter": mission_counter, "pending_exit_commands": pending_exit_commands,
    }))
}

/// Observe the already-installed owners before frame.rs admits sim_tick. No
/// bootstrap choices, country defaults, RNG draws or camera writes occur here.
fn campaign_runtime_receipt(state: &AppState) -> Result<Value> {
    ensure!(
        state
            .match_state
            .sim_runtime
            .as_ref()
            .is_some_and(|runtime| runtime.simulation.session().tick() == 0),
        "first live capture passed simulation tick zero"
    );
    installed_campaign_runtime_receipt(state)
}

fn initial_save_receipt(state: &AppState, directory: &Path) -> Result<Value> {
    let runtime = state
        .match_state
        .sim_runtime
        .as_ref()
        .context("initial save has no world")?;
    ensure!(
        runtime.simulation.session().tick() == 1,
        "initial save capture passed its first Logic step"
    );
    let entries = state.persistence.repository.browser_entries();
    ensure!(
        entries.len() == 1,
        "initial save capture expected one automatic file, got {}",
        entries.len()
    );
    let entry = &entries[0].0;
    ensure!(
        entry.path.parent() == Some(directory),
        "initial save escaped capture directory"
    );
    ensure!(
        entry.header.tick == 0,
        "automatic save was written after Logic"
    );
    let saved = crate::sim::snapshot::GameSnapshot::load_with_resources(
        &state.persistence.repository.read(&entry.path)?,
    )?;
    ensure!(
        saved.sim.session().game_options().game_speed == 2,
        "automatic save preceded the Options input"
    );
    let pending_speed = saved.sim.pending_command_snapshot().iter().any(|command| {
        matches!(
            command.payload,
            crate::sim::command::Command::SetGameSpeed { .. }
        )
    });
    ensure!(
        !pending_speed,
        "automatic save retained already-consumed Options input"
    );
    let browser =
        App::open_saved_game_rows(state, crate::ui::skirmish_shell::SavedSeedMode::Save, 8);
    ensure!(
        browser
            .selected_entry()
            .is_some_and(|row| row.file_name.is_none()),
        "Save did not select its new slot"
    );
    Ok(json!({
        "filename": entry.path.file_name().unwrap().to_string_lossy(),
        "description": entry.header.description, "tick": entry.header.tick,
        "map_name": entry.header.map_name,
        "game_speed": saved.sim.session().game_options().game_speed,
        "pending_speed_input": pending_speed,
        "new_save_description": browser.description_edit.display_text(),
        "observation": "ordinary save browser model; dialog not opened",
    }))
}

fn installed_campaign_runtime_receipt(state: &AppState) -> Result<Value> {
    let runtime = state
        .match_state
        .sim_runtime
        .as_ref()
        .context("first live frame has no runtime")?;
    let sim = &runtime.simulation;
    ensure!(
        !sim.session().game_mode_nonzero(),
        "campaign capture admitted a multiplayer session"
    );
    let mission_counter = sim
        .session()
        .campaign_mission_counter()
        .context("runtime has no campaign mission counter")?;
    let (player_difficulty, computer_difficulty) = sim
        .session()
        .campaign_house_difficulties()
        .context("runtime has no campaign difficulty rows")?;
    let current = sim
        .session()
        .current_house()
        .context("campaign has no current House")?;
    let current_house = sim
        .houses()
        .get(&current)
        .context("current House is not in the live roster")?;
    let current_native_id = current_house
        .native_unique_id()
        .context("current House has no native identity")?;
    let roster = sim.houses().order().iter().map(|id| {
        let house = sim.houses().get(id).context("registered House missing from live roster")?;
        let native_id = house.native_unique_id().context("registered House has no native identity")?;
        let team_timer = house.team_creation().timer();
        let attack_timer = house.native_attack_timer().map(|(timer, value)| json!({
            "start_frame": timer.start_frame(), "duration": timer.duration(), "value": value}));
        Ok(json!({"name": sim.interner().resolve(*id), "native_unique_id": native_id,
            "house_type": sim.interner().resolve(house.house_type_id()),
            "country": house.country().map(|country| sim.interner().resolve(country)),
            "side_index": house.side_index(), "is_human": house.is_human(),
            "player_control": house.player_control(), "difficulty": house.difficulty() as i32,
            "credits": house.economy().credits(), "scenario_credits": house.economy().scenario_credits(),
            "tech_level": house.tech_level(), "authored_iq": house.authored_iq(), "current_iq": house.current_iq(),
            "authored_edge": house.authored_edge(), "scenario_team_ratios": house.scenario_team_ratios(),
            "scalar_difficulty_bits": house.scalar_difficulty_biases().map(|value| format!("{:016x}", value.bits())),
            "attack_timer": attack_timer,
            "team_creation": {"ratio": house.team_creation().ratio(),
                "start_frame": team_timer.start_frame(), "duration": team_timer.duration()},
        }))
    }).collect::<Result<Vec<_>>>()?;
    let flags = sim.retained_campaign_special_flags();
    let rng = sim.rng_state();
    let rng_receipt = |stream: &crate::sim::rng::SimRngLogicalState| {
        json!({
            "disabled": stream.disabled, "index_a": stream.index_a, "index_b": stream.index_b,
            "words": &stream.words[..],
        })
    };
    let rules = &runtime.resources.rules;
    let player_buildings: Vec<_> = sim.entities().iter_sorted().filter_map(|(id, entity)| {
        (entity.owner() == current && entity.category() == crate::map::entities::EntityCategory::Structure)
            .then(|| json!({
                "id": id, "type": sim.interner().resolve(entity.type_ref()),
                "cell": [entity.position.rx(), entity.position.ry()],
                "in_playfield": entity.in_playfield(),
                "discovered": entity.discovery.discovered_by_current_house(),
                "opened": entity.building_art.actually_placed(),
                "ready": entity.building_ready_latch(),
                "mission": entity.mission.current().raw(), "queued": entity.mission.queued().raw(),
            }))
    }).collect();
    Ok(json!({
        "tick": sim.session().tick(), "binary_frame": sim.session().binary_frame(),
        "scenario_started": sim.session().started(),
        "startup_admission": {"accepted_rust_l0": state.match_state.startup.accepted().is_some(),
            "campaign_installation": state.match_state.startup.campaign_installed(),
            "saved_world_restored": state.match_state.startup.restored()},
        "seed": sim.session().seed(), "map_name": sim.session().map_name(), "theater": sim.session().theater(),
        "game_mode_nonzero": sim.session().game_mode_nonzero(), "campaign_mission_counter": mission_counter,
        "campaign_difficulty_rows": {"player": player_difficulty as i32, "computer": computer_difficulty as i32},
        "current_house": {"name": sim.interner().resolve(current), "native_unique_id": current_native_id},
        "flags": {"inert": flags.inert, "tiberium_grows": flags.tiberium_grows,
            "tiberium_spreads": flags.tiberium_spreads, "destroyable_bridges": flags.destroyable_bridges,
            "free_radar": sim.session().free_radar(), "ignore_global_ai_triggers": sim.session().ignore_global_ai_triggers()},
        "active_rules": {"source_ini_hash": format!("{:016x}", rules.source_ini_hash()),
            "simulation_config_hash": format!("{:016x}", rules.simulation_config_hash()),
            "domain": "parsed Rules projection and semantic config; not retail byte SHA256"},
        "house_roster": roster,
        "campaign_names": {"name_utf16": state.match_state.campaign_names.name_units(),
            "ui_name": state.match_state.campaign_names.ui_name(),
            "save_title_utf16": state.match_state.campaign_names.save_title_units(),
            "save_title": state.match_state.campaign_names.save_title()},
        "player_buildings": player_buildings,
        "scalar_difficulty_order": ["Firepower", "Groundspeed", "Airspeed", "Armor", "ROF", "Cost", "BuildTime", "RepairDelay", "BuildDelay"],
        "native_identity_cursor": sim.native_identity_cursor(),
        "rng": {"main": rng_receipt(&rng.main), "scenario": rng_receipt(&rng.scenario), "mapgen": rng_receipt(&rng.mapgen)},
        "camera": {"world_pixel_origin": [state.match_state.input.camera_x, state.match_state.input.camera_y],
            "zoom": state.match_state.input.zoom_level,
            "tactical_centre_cell": crate::app::input::camera::tactical_centre_cell(state),
            "view_bookmarks": (0..crate::app::input::camera::VIEW_BOOKMARK_SLOTS)
                .map(|slot| state.match_state.input.view_bookmarks.get(slot)).collect::<Vec<_>>()},
    }))
}
