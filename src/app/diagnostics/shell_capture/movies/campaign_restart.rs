//! Production campaign restart captures from the first live campaign frame:
//! B6 with Restart shown, B6 Restart into the restarted mission, and a forced
//! defeat through the Lose movie to the `TXT_TO_REPLAY` box (optionally Yes).
use super::*;
use crate::ui::shell::abort::AbortButton;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartTarget {
    /// B6 presented in a campaign mission, Restart visible.
    AbortShown,
    /// B6 Restart pressed; the restarted mission's first live frame.
    AbortRestarted,
    /// A forced loss's `TXT_TO_REPLAY` box over the empty backdrop.
    DefeatPrompt,
    /// Yes on that box; the restarted mission's first live frame.
    DefeatReplayed,
}

impl RestartTarget {
    pub(super) fn timeout(self) -> Duration {
        match self {
            Self::AbortShown => Duration::from_secs(90),
            _ => Duration::from_secs(240),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Abort,
    Settling(u32),
    AwaitDefeatPrompt,
    YesPressed,
    AwaitRestartedLive,
    Done,
}

/// The installed mission the route restarts, observed at its first live frame.
#[derive(Debug, Clone)]
struct Mission {
    map_name: String,
    campaign_index: usize,
    mission_counter: i32,
    seed: u64,
    /// The session's Options.Difficulty row and the player House's row,
    /// which the carry-over restore (`0x004C6295`) may replace.
    menu_difficulty: Option<i32>,
    player_difficulty: Option<i32>,
}

impl Mission {
    fn observe(state: &AppState) -> Result<Self> {
        let startup = state
            .match_state
            .startup
            .installed_campaign()
            .context("no installed campaign mission")?;
        let sim = &state
            .match_state
            .sim_runtime
            .as_ref()
            .context("no live campaign runtime")?
            .simulation;
        Ok(Self {
            map_name: sim.session().map_name().to_owned(),
            campaign_index: startup.campaign_index(),
            mission_counter: sim
                .session()
                .campaign_mission_counter()
                .context("runtime has no campaign mission counter")?,
            seed: sim.session().seed(),
            menu_difficulty: sim
                .session()
                .campaign_house_difficulties()
                .map(|(player, _)| player as i32),
            player_difficulty: sim
                .session()
                .current_house()
                .and_then(|player| sim.houses().get(&player))
                .map(|house| house.difficulty() as i32),
        })
    }

    fn receipt(&self) -> Value {
        json!({"map_name": self.map_name, "campaign_index": self.campaign_index,
            "mission_counter": self.mission_counter, "seed": self.seed,
            "menu_difficulty_row": self.menu_difficulty,
            "player_house_difficulty_row": self.player_difficulty})
    }
}

pub(super) struct RestartCapture {
    target: RestartTarget,
    step: Step,
    abort: Option<abort_return::AbortReturnInput>,
    before: Mission,
    after: Option<Mission>,
    observed_restart_loading: Option<Value>,
}

impl RestartCapture {
    pub(super) fn begin(
        target: RestartTarget,
        state: &mut AppState,
        route: &mut Vec<Value>,
        frame: u32,
    ) -> Result<Self> {
        let before = Mission::observe(state)?;
        route.push(
            json!({"frame": frame, "action": "first live observed", "mission": before.receipt()}),
        );
        let mut capture = Self {
            target,
            step: Step::Abort,
            abort: None,
            before,
            after: None,
            observed_restart_loading: None,
        };
        match target {
            RestartTarget::AbortShown | RestartTarget::AbortRestarted => {
                let choice =
                    (target == RestartTarget::AbortRestarted).then_some(AbortButton::Secondary);
                capture.abort = Some(abort_return::AbortReturnInput::begin_choosing(
                    state, route, frame, choice,
                )?);
            }
            RestartTarget::DefeatPrompt | RestartTarget::DefeatReplayed => {
                Self::lose_current_house(state, route, frame)?;
                capture.step = Step::AwaitDefeatPrompt;
            }
        }
        Ok(capture)
    }

    /// A map's TAction 68 (Lose) through the House outcome owner; the
    /// SavourDelay, outcome EVA, defeat cascade and GameExit::Defeat tail
    /// then run unaided.
    #[cfg(feature = "dev-overlays")]
    fn lose_current_house(state: &mut AppState, route: &mut Vec<Value>, frame: u32) -> Result<()> {
        let runtime = state
            .match_state
            .sim_runtime
            .as_mut()
            .context("no live campaign runtime")?;
        ensure!(
            runtime
                .simulation
                .dev_trigger_current_house_lose(&runtime.resources.rules),
            "TAction 68 did not flag the current House to lose"
        );
        route.push(
            json!({"frame": frame, "action": "TAction 68 Lose on the current House",
            "tick": runtime.simulation.session().tick()}),
        );
        Ok(())
    }

    #[cfg(not(feature = "dev-overlays"))]
    fn lose_current_house(_: &mut AppState, _: &mut Vec<Value>, _: u32) -> Result<()> {
        bail!("forcing a campaign defeat needs the dev-overlays build")
    }

    /// The restarted mission's first live frame holds tick zero for readback.
    pub(super) fn owns_simulation_steps(&self, state: &AppState) -> bool {
        self.step == Step::AwaitRestartedLive && state.frontend.screen == GameScreen::InGame
    }

    pub(super) fn after_present(
        &mut self,
        state: &mut AppState,
        rendered: PresentedShell,
        route: &mut Vec<Value>,
        frame: u32,
    ) -> Result<()> {
        if rendered == PresentedShell::FullscreenMovie {
            // Lose and the restarted Intro/Brief: Play_Movie accepts only the
            // ordinary bare Escape release.
            let displayed_frame = state
                .frontend
                .fullscreen_movie
                .as_ref()
                .context("movie ended before its presented frame")?
                .displayed_frame();
            App::fullscreen_movie_key(state, winit::keyboard::KeyCode::Escape, true);
            App::fullscreen_movie_key(state, winit::keyboard::KeyCode::Escape, false);
            route.push(json!({"frame": frame, "action": "movie Escape release",
                "movie_frame": displayed_frame}));
            return Ok(());
        }
        match self.step {
            Step::Abort => {
                let done = self
                    .abort
                    .as_mut()
                    .context("abort input owner")?
                    .after_present(state, route, frame)?;
                if done {
                    self.step = match self.target {
                        RestartTarget::AbortShown => Step::Settling(SETTLE_FRAMES),
                        _ => Step::AwaitRestartedLive,
                    };
                }
            }
            Step::Settling(remaining) if remaining > 0 => {
                self.step = Step::Settling(remaining - 1);
            }
            Step::Settling(_) | Step::Done => {}
            Step::AwaitDefeatPrompt => {
                let replay_shown = matches!(
                    state
                        .frontend
                        .shell_message_box
                        .as_ref()
                        .map(|message| &message.purpose),
                    Some(
                        crate::app::frontend::state::ShellMessageBoxPurpose::CampaignReplay(Some(
                            _
                        ))
                    )
                );
                if replay_shown && state.frontend.screen == GameScreen::MainMenu {
                    route.push(
                        json!({"frame": frame, "action": "TXT_TO_REPLAY shown", "dialog": 0x120}),
                    );
                    if self.target == RestartTarget::DefeatPrompt {
                        Self::restore_pointer(state);
                        self.step = Step::Settling(SETTLE_FRAMES);
                    } else {
                        let layout = crate::ui::shell::modal::quit_confirm_layout(
                            state.renderer.gpu.config.width as i32,
                            state.renderer.gpu.config.height as i32,
                        );
                        let point = set_window_pointer_to_rect_center(state, layout.ok);
                        App::handle_shell_message_box_mouse_down(state);
                        ensure!(
                            state.frontend.shell_controller.pressed()
                                == Some(crate::ui::shell::modal::control::OK),
                            "physical Yes press missed its loaded control"
                        );
                        route.push(
                            json!({"frame": frame, "action": "Yes press", "dialog": 0x120,
                            "point": point}),
                        );
                        self.step = Step::YesPressed;
                    }
                }
            }
            Step::YesPressed => {
                App::handle_shell_message_box_mouse_up(state);
                ensure!(
                    state.frontend.shell_message_box.is_none(),
                    "physical Yes release did not answer the box"
                );
                route.push(json!({"frame": frame, "action": "Yes release", "dialog": 0x120}));
                Self::restore_pointer(state);
                self.step = Step::AwaitRestartedLive;
            }
            Step::AwaitRestartedLive => {
                if let Some(startup) = crate::app::loading::pump::loading_campaign_startup(state)
                    && self.observed_restart_loading.is_none()
                {
                    ensure!(
                        !startup.plays_action_movie(),
                        "the restart passed Start_Scenario DL 1"
                    );
                    self.observed_restart_loading = Some(json!({"frame": frame,
                        "scenario": startup.scenario(), "campaign_index": startup.campaign_index(),
                        "mission_counter": startup.mission_counter(),
                        "plays_action_movie": startup.plays_action_movie()}));
                    route.push(json!({"frame": frame, "action": "restart loading admitted"}));
                }
                if state.frontend.screen == GameScreen::InGame && self.after.is_none() {
                    ensure!(
                        self.observed_restart_loading.is_some(),
                        "the restarted mission was not observed loading"
                    );
                    let after = Mission::observe(state)?;
                    ensure!(
                        after.map_name.eq_ignore_ascii_case(&self.before.map_name)
                            && after.campaign_index == self.before.campaign_index
                            && after.mission_counter == self.before.mission_counter,
                        "the restart did not reinstall the same mission"
                    );
                    ensure!(
                        state
                            .match_state
                            .sim_runtime
                            .as_ref()
                            .is_some_and(|runtime| runtime.simulation.session().tick() == 0),
                        "restarted first live frame passed tick zero"
                    );
                    route.push(
                        json!({"frame": frame, "action": "restarted first live observed",
                        "mission": after.receipt()}),
                    );
                    self.after = Some(after);
                    self.step = Step::Done;
                }
            }
        }
        Ok(())
    }

    fn restore_pointer(state: &mut AppState) {
        MoviesCapture::restore_neutral_pointer(state);
    }

    pub(super) fn ready(&self, state: &AppState) -> Result<bool> {
        Ok(match self.target {
            RestartTarget::AbortShown => {
                self.step == Step::Settling(0)
                    && state.match_state.match_presentation.in_game_menu
                        == crate::ui::pause_menu::InGameMenuState::AbortConfirm
                    && crate::app::input::abort::current_abort_layout(state)
                        .is_some_and(|layout| layout.secondary.is_some())
            }
            RestartTarget::DefeatPrompt => {
                self.step == Step::Settling(0) && state.frontend.shell_message_box.is_some()
            }
            RestartTarget::AbortRestarted | RestartTarget::DefeatReplayed => {
                self.step == Step::Done && state.frontend.screen == GameScreen::InGame
            }
        })
    }

    pub(super) fn receipt(&self) -> Value {
        json!({"target": format!("{:?}", self.target), "before": self.before.receipt(),
            "restart_loading": self.observed_restart_loading,
            "after": self.after.as_ref().map(Mission::receipt)})
    }
}
