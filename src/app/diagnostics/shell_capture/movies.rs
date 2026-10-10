//! Production-only main-menu family capture routes (Movies & Credits, the
//! Exit confirmation and campaign selection). Route actions go through the
//! ordinary main-menu, `0x100`, `0x101`, `0x129` and `0x94` handlers after a
//! presented frame; no shell state or renderer is cloned.

use super::*;
use crate::app::App;
use crate::ui::campaign_shell::CampaignSide;
use serde_json::{Value, json};

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
    },
    /// Network on `0xE2`: its teardown slide, then a new `0xE2` with its
    /// own entry slide, captured once settled.
    NetworkBounce,
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
    AbortReturn,
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
    CampaignReturning,
    CampaignAbortMenu,
    CampaignAbortPressed,
    CampaignAbortModal,
    CampaignLeavePressed,
    CampaignLeaving,
    LoadSavedGame,
    Options,
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
}

impl MoviesCapture {
    pub(super) fn new(target: MoviesTarget) -> Self {
        Self {
            target,
            phase: Phase::MainMenu,
            route: Vec::new(),
            observed_campaign_startup: None,
            observed_campaign_runtime: None,
            observed_exit_cascade: false,
        }
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
            Phase::CampaignAbortMenu
                | Phase::CampaignAbortPressed
                | Phase::CampaignAbortModal
                | Phase::CampaignLeavePressed
                | Phase::CampaignLeaving
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
                // This is the ordinary Escape-press owner at handler.rs808..819;
                // in_game_key_edge alone would skip its modal admission branch.
                ensure!(
                    state.platform.live_modifiers.is_empty(),
                    "pause menu requires bare Escape"
                );
                ensure!(
                    App::in_game_menu_owns_escape(state),
                    "Escape belongs to an armed tactical mode"
                );
                App::route_in_game_menu_escape(state);
                ensure!(
                    state.match_state.match_presentation.in_game_menu
                        == crate::ui::pause_menu::InGameMenuState::Menu,
                    "ordinary Escape did not open B5"
                );
                self.route
                    .push(json!({"frame": frame, "action": "Escape press", "dialog": 0xb5}));
                self.phase = Phase::CampaignAbortMenu;
            }
            return Ok(());
        }
        if aborting_campaign {
            return self.campaign_abort_after_present(state, frame);
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
                    if matches!(self.target, MoviesTarget::Options0xD5 { .. }) {
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
                let MoviesTarget::Options0xD5 { entry_tick, hover } = self.target else {
                    bail!("options phase without an options target");
                };
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
                    && Self::slide_settled(state, ShellSlideKind::Options)
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
                            state
                                .frontend
                                .wol_welcome
                                .as_ref()
                                .is_some_and(|wol| wol.api_missing.is_some()),
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
                if state.frontend.exit_confirm_modal.is_some()
                    && state.frontend.shell_exit.is_none()
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
        use crate::ui::pause_menu::InGameMenuState;
        use crate::ui::shell::abort::AbortButton;
        use crate::ui::shell::pause_menu::PauseMenuButton;
        use winit::event::{ElementState, MouseButton};
        match self.phase {
            Phase::CampaignAbortMenu => {
                ensure!(
                    state.match_state.match_presentation.in_game_menu == InGameMenuState::Menu
                        && crate::app::frontend::skirmish_shell_render::native_in_game_shell_active(
                            state
                        ),
                    "campaign B5 did not present with its required loaded artwork"
                );
                // The window handler's modal branch consumes the press. Its
                // paused Escape release reaches this existing KeyEdge owner.
                let escape = winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape);
                let paused_at_event = state.match_state.paused();
                crate::app::input::keyboard::in_game_key_edge(
                    state,
                    crate::app::input::keyboard::InGameKeyEdge {
                        physical: winit::keyboard::KeyCode::Escape,
                        logical: &escape,
                        unmodified: &escape,
                        location: winit::keyboard::KeyLocation::Standard,
                        state: ElementState::Released,
                        repeat: false,
                    },
                    false,
                    paused_at_event,
                );
                self.route
                    .push(json!({"frame": frame, "action": "Escape release", "dialog": 0xb5}));
                self.route.push(
                    json!({"frame": frame, "action": "pause menu presented", "dialog": 0xb5}),
                );
                let (shell, size) =
                    crate::app::frontend::skirmish_shell_render::current_in_game_shell_layout(
                        state,
                    )
                    .context("campaign B5 has no loaded hit geometry")?;
                let layout = crate::ui::shell::pause_menu::pause_menu_layout(
                    state.renderer.gpu.config.width as i32,
                    state.renderer.gpu.config.height as i32,
                    shell,
                    size,
                );
                let rect = layout.buttons[PauseMenuButton::Abort as usize];
                let point = set_window_pointer_to_rect_center(state, rect);
                crate::app::input::dispatch::handle_cursor_moved_in_game(state);
                crate::app::input::dispatch::handle_mouse_input(
                    state,
                    MouseButton::Left,
                    ElementState::Pressed,
                );
                ensure!(
                    state
                        .match_state
                        .match_presentation
                        .pause_menu_interaction
                        .is_pressed(PauseMenuButton::Abort),
                    "physical B5 Abort press missed its loaded control"
                );
                self.route.push(
                    json!({"frame": frame, "action": "Abort press", "dialog": 0xb5,
                    "point": point, "rect": [rect.x, rect.y, rect.w, rect.h]}),
                );
                self.phase = Phase::CampaignAbortPressed;
            }
            Phase::CampaignAbortPressed => {
                ensure!(
                    state.match_state.match_presentation.in_game_menu == InGameMenuState::Menu
                        && state
                            .match_state
                            .match_presentation
                            .pause_menu_interaction
                            .is_pressed(PauseMenuButton::Abort),
                    "B5 Abort capture changed before its presented press/release"
                );
                crate::app::input::dispatch::handle_mouse_input(
                    state,
                    MouseButton::Left,
                    ElementState::Released,
                );
                ensure!(
                    state.match_state.match_presentation.in_game_menu
                        == InGameMenuState::AbortConfirm,
                    "physical B5 Abort release did not open B6"
                );
                self.route
                    .push(json!({"frame": frame, "action": "Abort release", "dialog": 0xb5}));
                self.phase = Phase::CampaignAbortModal;
            }
            Phase::CampaignAbortModal => {
                ensure!(
                    state.match_state.match_presentation.in_game_menu
                        == InGameMenuState::AbortConfirm
                        && crate::app::frontend::skirmish_shell_render::native_in_game_shell_active(
                            state
                        ),
                    "campaign B6 did not present with its required loaded artwork"
                );
                self.route.push(
                    json!({"frame": frame, "action": "abort modal presented", "dialog": 0xb6}),
                );
                let (shell, size) =
                    crate::app::frontend::skirmish_shell_render::current_in_game_shell_layout(
                        state,
                    )
                    .context("campaign B6 has no loaded hit geometry")?;
                let layout = crate::ui::shell::abort::abort_layout(
                    state.renderer.gpu.config.width as i32,
                    state.renderer.gpu.config.height as i32,
                    shell,
                    size,
                );
                let rect = layout.button(AbortButton::Leave);
                let point = set_window_pointer_to_rect_center(state, rect);
                crate::app::input::dispatch::handle_cursor_moved_in_game(state);
                crate::app::input::dispatch::handle_mouse_input(
                    state,
                    MouseButton::Left,
                    ElementState::Pressed,
                );
                ensure!(
                    state
                        .match_state
                        .match_presentation
                        .abort_buttons
                        .is_pressed(AbortButton::Leave),
                    "physical B6 Leave press missed its loaded control"
                );
                self.route.push(
                    json!({"frame": frame, "action": "Leave press", "dialog": 0xb6,
                    "point": point, "rect": [rect.x, rect.y, rect.w, rect.h]}),
                );
                self.phase = Phase::CampaignLeavePressed;
            }
            Phase::CampaignLeavePressed => {
                ensure!(
                    state.match_state.match_presentation.in_game_menu
                        == InGameMenuState::AbortConfirm
                        && state
                            .match_state
                            .match_presentation
                            .abort_buttons
                            .is_pressed(AbortButton::Leave),
                    "B6 Leave capture changed before its presented press/release"
                );
                let runtime = state
                    .match_state
                    .sim_runtime
                    .as_ref()
                    .context("B6 lost its installed runtime")?;
                ensure!(
                    !runtime
                        .simulation
                        .pending_command_snapshot()
                        .iter()
                        .any(|command| matches!(
                            command.payload,
                            crate::sim::command::Command::ExitMatch
                        )),
                    "an EXIT was already queued before physical Leave release"
                );
                crate::app::input::dispatch::handle_mouse_input(
                    state,
                    MouseButton::Left,
                    ElementState::Released,
                );
                ensure!(
                    state.match_state.match_presentation.in_game_menu == InGameMenuState::Closed
                        && !state.match_state.paused(),
                    "physical Leave did not release the ordinary modal pause"
                );
                let sim = &state
                    .match_state
                    .sim_runtime
                    .as_ref()
                    .context("Leave lost its installed runtime")?
                    .simulation;
                let exits = sim
                    .pending_command_snapshot()
                    .into_iter()
                    .filter(|command| {
                        matches!(command.payload, crate::sim::command::Command::ExitMatch)
                    })
                    .collect::<Vec<_>>();
                ensure!(
                    exits.len() == 1 && Some(exits[0].owner) == sim.session.current_house(),
                    "physical Leave did not queue exactly the current House's EXIT"
                );
                let exit = &exits[0];
                ensure!(
                    !sim.quit_requested,
                    "Leave confirmation executed EXIT instead of only queueing it"
                );
                self.route
                    .push(json!({"frame": frame, "action": "Leave release", "dialog": 0xb6}));
                self.route.push(json!({"frame": frame, "action": "EXIT queued", "queued_at_tick": sim.session.tick,
                    "execute_tick": exit.execute_tick, "owner": sim.interner.resolve(exit.owner),
                    "owner_native_unique_id": sim.houses.get(&exit.owner).and_then(|house| house.native_unique_id()),
                    "modal_closed": !state.match_state.paused(), "diagnostic_simulation_freeze": false,
                    "quit_requested": sim.quit_requested}));
                Self::restore_neutral_pointer(state);
                self.phase = Phase::CampaignLeaving;
            }
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
                        sim.quit_requested
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
                        json!({"frame": frame, "action": "EXIT consumed", "tick": sim.session.tick,
                        "scenario_exit_present": true, "scenario_outcome_present": false,
                        "quit_requested": sim.quit_requested}),
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
        if let MoviesTarget::CampaignStart { side, target } = self.target {
            if self.phase != Phase::CampaignStarting {
                return Ok(false);
            }
            if let Some(startup) = crate::app::loading::pump::loading_campaign_startup(state) {
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
                CampaignStartTarget::FirstLive => {
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
                CampaignStartTarget::AbortReturn => unreachable!("handled above"),
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
            MoviesTarget::ExitConfirm => state.frontend.exit_confirm_modal.is_some(),
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
            } => state
                .frontend
                .wol_welcome
                .as_ref()
                .is_some_and(|wol| wol.api_missing.is_some()),
            MoviesTarget::WolWelcome { entry_tick, .. } => {
                state.frontend.shell_route.wol_welcome()
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
            MoviesTarget::Options0xD5 { entry_tick, .. } => {
                state.frontend.options_dialog.is_some()
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
                CampaignStartTarget::FirstLive | CampaignStartTarget::AbortReturn => {
                    state.match_state.loaded_map_source.as_ref()
                }
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
                CampaignStartTarget::AbortReturn => "abort-return",
            });
            manifest["runtime"] = match target {
                CampaignStartTarget::LoadingFirst => Value::Null,
                CampaignStartTarget::FirstLive => campaign_runtime_receipt(state)?,
                CampaignStartTarget::AbortReturn => self
                    .observed_campaign_runtime
                    .clone()
                    .context("campaign abort has no pre-Escape runtime observation")?,
            };
            if target == CampaignStartTarget::AbortReturn {
                manifest["runtime_observation"] = json!("first-live-frame-before-Escape-press");
                manifest["cleanup"] = campaign_abort_cleanup_receipt(state)?;
            }
            manifest["coverage"] = json!({
                "loading_readback": "first presented loading screen only; later synchronous milestones are not captured",
                "first_live": "first InGame draw before any simulation tick",
                "mission_opening_1308": "unresolved; this receipt does not certify scripted mission opening",
            });
            if target == CampaignStartTarget::AbortReturn {
                manifest["coverage"]["abort_audio"] =
                    json!("ordinary exit cascade observed; device audio output is not captured");
                manifest["coverage"]["campaign_b6_restart"] =
                    json!("not implemented by the existing abort owner; selected Leave path only");
            }
        } else if self.target == MoviesTarget::CampaignBack {
            manifest["schema_version"] = json!("vera20k.campaign-start-capture.v1");
            manifest["checkpoint_phase"] = json!("campaign-back-return");
            manifest["shell_return"] = json!({"screen": format!("{:?}", state.frontend.screen),
                "route": format!("{:?}", state.frontend.shell_route),
                "campaign_page_present": state.frontend.campaign.is_some(),
                "loading_session_present": state.frontend.loading_session.is_some()});
        }
        Ok(manifest)
    }

    pub(super) fn freezes_simulation_for_first_live(&self, state: &AppState) -> bool {
        matches!(
            self.target,
            MoviesTarget::CampaignStart {
                target: CampaignStartTarget::FirstLive | CampaignStartTarget::AbortReturn,
                ..
            }
        ) && self.phase == Phase::CampaignStarting
            && state.frontend.screen == GameScreen::InGame
    }
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

fn campaign_startup_receipt(startup: &crate::match_bootstrap::PreparedCampaignStartup) -> Value {
    let campaign = startup.campaign();
    json!({
        "campaign": {"id": campaign.id(), "index": startup.campaign_index(), "cd": campaign.cd(),
            "scenario": campaign.scenario(), "final_movie": campaign.final_movie()},
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
        .session
        .campaign_mission_counter()
        .context("retained runtime lost campaign state")?;
    let pending_exit_commands = sim
        .pending_command_snapshot()
        .iter()
        .filter(|command| matches!(command.payload, crate::sim::command::Command::ExitMatch))
        .count();
    ensure!(
        mission_counter == 1 && sim.quit_requested && pending_exit_commands == 0,
        "campaign return did not consume EXIT/reset the retained Session counter"
    );
    Ok(json!({
        "screen": format!("{:?}", state.frontend.screen), "route": format!("{:?}", state.frontend.shell_route),
        "loading_session_present": state.frontend.loading_session.is_some(),
        "loading_campaign_startup_present": crate::app::loading::pump::loading_campaign_startup(state).is_some(),
        "accepted_match_startup_present": state.match_state.startup.startup().is_some(),
        "match_startup_receipt_present": state.match_state.startup.receipt().is_some(),
        "asset_manager_leased": state.process_assets.is_leased(), "asset_manager_available": state.process_assets.is_available(),
        "campaign_page_present": state.frontend.campaign.is_some(), "fullscreen_movie_present": state.frontend.fullscreen_movie.is_some(),
        "scenario_exit_present": state.match_state.scenario_exit.is_some(), "scenario_outcome_present": state.match_state.scenario_outcome.is_some(),
        "in_game_menu": format!("{:?}", state.match_state.match_presentation.in_game_menu),
        "retained_runtime_present": state.match_state.sim_runtime.is_some(), "retained_tick": sim.session.tick,
        "retained_quit_requested": sim.quit_requested,
        "campaign_mission_counter": mission_counter, "pending_exit_commands": pending_exit_commands,
    }))
}

/// Observe the already-installed owners before frame.rs admits sim_tick. No
/// bootstrap choices, country defaults, RNG draws or camera writes occur here.
fn campaign_runtime_receipt(state: &AppState) -> Result<Value> {
    let runtime = state
        .match_state
        .sim_runtime
        .as_ref()
        .context("first live frame has no runtime")?;
    let sim = &runtime.simulation;
    ensure!(
        sim.session.tick == 0,
        "first live capture passed simulation tick zero"
    );
    ensure!(
        !sim.session.game_mode_nonzero,
        "campaign capture admitted a multiplayer session"
    );
    let mission_counter = sim
        .session
        .campaign_mission_counter()
        .context("runtime has no campaign mission counter")?;
    let (player_difficulty, computer_difficulty) = sim
        .session
        .campaign_house_difficulties()
        .context("runtime has no campaign difficulty rows")?;
    let current = sim
        .session
        .current_house()
        .context("campaign has no current House")?;
    let current_house = sim
        .houses
        .get(&current)
        .context("current House is not in the live roster")?;
    let current_native_id = current_house
        .native_unique_id()
        .context("current House has no native identity")?;
    let roster = sim.session.house_order.iter().map(|id| {
        let house = sim.houses.get(id).context("registered House missing from live roster")?;
        let native_id = house.native_unique_id().context("registered House has no native identity")?;
        let team_timer = house.team_creation.timer();
        let attack_timer = house.native_attack_timer().map(|(timer, value)| json!({
            "start_frame": timer.start_frame(), "duration": timer.duration(), "value": value}));
        Ok(json!({"name": sim.interner.resolve(*id), "native_unique_id": native_id,
            "house_type": sim.interner.resolve(house.house_type_id()),
            "country": house.country.map(|country| sim.interner.resolve(country)),
            "side_index": house.side_index, "is_human": house.is_human,
            "player_control": house.player_control, "difficulty": house.difficulty as i32,
            "credits": house.economy.credits(), "scenario_credits": house.economy.scenario_credits(),
            "tech_level": house.tech_level, "authored_iq": house.authored_iq, "current_iq": house.current_iq,
            "authored_edge": house.authored_edge(), "scenario_team_ratios": house.scenario_team_ratios(),
            "scalar_difficulty_bits": house.scalar_difficulty_biases().map(|value| format!("{:016x}", value.bits())),
            "attack_timer": attack_timer,
            "team_creation": {"ratio": house.team_creation.ratio(),
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
    Ok(json!({
        "tick": sim.session.tick, "binary_frame": sim.session.binary_frame,
        "seed": sim.session.seed, "map_name": sim.session.map_name, "theater": sim.session.theater,
        "game_mode_nonzero": sim.session.game_mode_nonzero, "campaign_mission_counter": mission_counter,
        "campaign_difficulty_rows": {"player": player_difficulty as i32, "computer": computer_difficulty as i32},
        "current_house": {"name": sim.interner.resolve(current), "native_unique_id": current_native_id},
        "flags": {"inert": flags.inert, "tiberium_grows": flags.tiberium_grows,
            "tiberium_spreads": flags.tiberium_spreads, "destroyable_bridges": flags.destroyable_bridges,
            "free_radar": sim.session.free_radar, "ignore_global_ai_triggers": sim.session.ignore_global_ai_triggers()},
        "active_rules": {"source_ini_hash": format!("{:016x}", rules.source_ini_hash()),
            "simulation_config_hash": format!("{:016x}", rules.simulation_config_hash()),
            "domain": "parsed Rules projection and semantic config; not retail byte SHA256"},
        "house_roster": roster,
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
