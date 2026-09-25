//! Production-only main-menu family capture routes (Movies & Credits, the
//! Exit confirmation and campaign selection). Route actions go through the
//! ordinary main-menu, `0x100`, `0x101`, `0x129` and `0x94` handlers after a
//! presented frame; no shell state or renderer is cloned.

use super::*;
use crate::app::App;
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
    /// press on the difficulty slider at `press`, or held at `entry_tick` of
    /// its entry slide.
    Campaign0x94 {
        press: Option<(i32, i32)>,
        entry_tick: Option<u32>,
    },
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
}

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
    LoadSavedGame,
    Options,
    /// Network pressed: waiting for the teardown to commit.
    NetworkLeaving,
    /// A new `0xE2` exists: waiting for its entry slide to be seen.
    NetworkReturning,
    /// The new `0xE2`'s entry slide was seen: waiting for steady paint.
    NetworkReturned,
    Settling(u32),
}

pub(super) struct MoviesCapture {
    target: MoviesTarget,
    phase: Phase,
    route: Vec<Value>,
}

impl MoviesCapture {
    pub(super) fn new(target: MoviesTarget) -> Self {
        Self {
            target,
            phase: Phase::MainMenu,
            route: Vec::new(),
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
        ensure!(
            state.frontend.screen == GameScreen::MainMenu,
            "movies capture left the shell"
        );
        ensure!(
            !state.frontend.main_menu_shell_failed,
            "movies capture encountered the shell fallback"
        );
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
                        MoviesTarget::Campaign0x94 { .. } | MoviesTarget::LoadSavedGame0xB7 { .. }
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
                        | MoviesTarget::LoadSavedGame0xB7 { .. }
                        | MoviesTarget::Options0xD5 { .. }
                        | MoviesTarget::NetworkBounce => {
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
                    if let MoviesTarget::Campaign0x94 {
                        press: Some(point), ..
                    } = self.target
                    {
                        // The native helper clicks, then recenters the pointer.
                        state.match_state.input.cursor_x = point.0 as f32;
                        state.match_state.input.cursor_y = point.1 as f32;
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

    pub(super) fn ready(&self, state: &AppState) -> Result<bool> {
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
            | MoviesTarget::SneakPeek { .. }
            | MoviesTarget::ExitConfirm
            | MoviesTarget::SlideOut { .. }
            | MoviesTarget::ListBackFirstFrame
            | MoviesTarget::NetworkBounce => true,
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
            MoviesTarget::NetworkBounce => {
                state.frontend.shell_exit.is_none()
                    && crate::app::frontend::shell_transition::current_shell_slide_target(state)
                        == Some(ShellSlideKind::MainMenu)
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
    ) -> Value {
        json!({
            "schema_version": "vera20k.movies-shell-capture.v1",
            "checkpoint": request.checkpoint.as_str(),
            "parity_certification": "NONE", "presenter_domain": "final-swapchain-after-rgb565",
            "surface": {"width": request.width, "height": request.height, "format": format!("{format:?}"),
                "pixel_layout": "BGRA8", "row_order": "top-left", "row_stride": request.width * 4},
            "cursor": {"x": request.cursor_x, "y": request.cursor_y, "policy": "software-composited"},
            "route": self.route, "capture_frame": frame,
            "frame": {"path": FRAME_FILE_NAME, "byte_length": pixels.len(),
                "sha256": crate::util::sha256::sha256_hex(pixels)}
        })
    }
}
