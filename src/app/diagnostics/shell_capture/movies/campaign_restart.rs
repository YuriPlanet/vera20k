//! Production campaign restart captures from the first live campaign frame:
//! B6 with Restart shown, B6 Restart into the restarted mission, and a forced
//! defeat through the Lose movie to the `TXT_TO_REPLAY` box (optionally Yes),
//! and the sidebar Briefing tab's mission restate 0x10C over that frame.
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
    /// No on that box; the ordinary shell and its INTRO theme.
    DefeatReturned,
    /// Return to the shell through the production keyboard admission owner.
    DefeatReturnedEnter,
    DefeatReturnedNumpadEnter,
    DefeatReturnedEscape,
    DefeatReturnedRepeatedEnter,
    /// The sidebar 0xF2 tab pressed and released: the restate briefing 0x10C.
    Restate,
    /// Save/load in game, then press the restored campaign Restart.
    SavedAbortRestarted,
    /// Save, return to the main menu, load, lose and accept replay.
    MenuDefeatReplayed,
    /// Save/load, win through TAction67, continue the score into ALL02.
    SavedVictory,
    /// SOV02's authored objective, held behind B5 past its wall deadline.
    ObjectiveExpiry,
}

impl RestartTarget {
    fn return_key(self) -> Option<(winit::keyboard::KeyCode, bool)> {
        use winit::keyboard::KeyCode;
        match self {
            Self::DefeatReturnedEnter => Some((KeyCode::Enter, false)),
            Self::DefeatReturnedNumpadEnter => Some((KeyCode::NumpadEnter, false)),
            Self::DefeatReturnedEscape => Some((KeyCode::Escape, false)),
            Self::DefeatReturnedRepeatedEnter => Some((KeyCode::Enter, true)),
            _ => None,
        }
    }

    pub(super) fn timeout(self) -> Duration {
        match self {
            Self::AbortShown | Self::Restate => Duration::from_secs(90),
            _ => Duration::from_secs(240),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Abort,
    Settling(u32),
    AwaitBattleTheme,
    AwaitDefeatPrompt,
    HoldDefeatPrompt(u32),
    AwaitVictoryScore,
    ReplayPressed,
    AwaitShellReturn,
    AwaitObjective,
    AwaitObjectiveMenu,
    HoldObjectiveMenu,
    ObjectiveResumed,
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

#[derive(Debug)]
struct ObjectivePause {
    text: String,
    deadline_ms: u64,
    paused_tick: u64,
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
    restored: Option<Value>,
    battle_theme: Option<i32>,
    theme_observations: Vec<Value>,
    objective_pause: Option<ObjectivePause>,
    objective_observations: Vec<Value>,
}

impl RestartCapture {
    pub(super) fn begin(
        target: RestartTarget,
        state: &mut AppState,
        route: &mut Vec<Value>,
        frame: u32,
        save_directory: &Path,
    ) -> Result<Self> {
        let before = Mission::observe(state)?;
        route.push(
            json!({"frame": frame, "action": "first live observed", "mission": before.receipt()}),
        );
        let restored = if matches!(
            target,
            RestartTarget::SavedAbortRestarted
                | RestartTarget::MenuDefeatReplayed
                | RestartTarget::SavedVictory
        ) {
            state.persistence.isolate_capture_saves(save_directory)?;
            let path =
                crate::app::persistence::commands::save_game(state, "Campaign continuation", None)
                    .context("save the campaign through the ordinary command")?;
            let from_menu = target == RestartTarget::MenuDefeatReplayed;
            if from_menu {
                App::return_to_main_menu(state);
                ensure!(
                    state.match_state.startup.installed_campaign().is_none(),
                    "menu return retained outgoing campaign admission"
                );
            }
            crate::app::persistence::commands::try_load_save_file(state, &path)
                .context("restore through the ordinary saved-game loader")?;
            let loaded = Mission::observe(state)?;
            ensure!(
                state.match_state.startup.restored(),
                "save load did not commit"
            );
            ensure!(
                loaded.map_name == before.map_name
                    && loaded.campaign_index == before.campaign_index
                    && loaded.mission_counter == before.mission_counter,
                "saved campaign identity changed"
            );
            Some(json!({"path": path, "from_menu": from_menu, "mission": loaded.receipt()}))
        } else {
            None
        };
        let mut capture = Self {
            target,
            step: Step::Abort,
            abort: None,
            before,
            after: None,
            observed_restart_loading: None,
            restored,
            battle_theme: None,
            theme_observations: Vec::new(),
            objective_pause: None,
            objective_observations: Vec::new(),
        };
        match target {
            RestartTarget::AbortShown
            | RestartTarget::AbortRestarted
            | RestartTarget::SavedAbortRestarted => {
                let choice =
                    (target != RestartTarget::AbortShown).then_some(AbortButton::Secondary);
                capture.abort = Some(abort_return::AbortReturnInput::begin_choosing(
                    state, route, frame, choice,
                )?);
            }
            RestartTarget::DefeatPrompt
            | RestartTarget::DefeatReplayed
            | RestartTarget::DefeatReturned
            | RestartTarget::DefeatReturnedEnter
            | RestartTarget::DefeatReturnedNumpadEnter
            | RestartTarget::DefeatReturnedEscape
            | RestartTarget::DefeatReturnedRepeatedEnter
            | RestartTarget::MenuDefeatReplayed => {
                capture.step = Step::AwaitBattleTheme;
            }
            RestartTarget::SavedVictory => {
                Self::complete_current_house(
                    state,
                    route,
                    frame,
                    crate::sim::house_state::HouseOutcomeKind::Victory,
                )?;
                capture.step = Step::AwaitVictoryScore;
            }
            RestartTarget::Restate => {
                Self::click_briefing_tab(state, route, frame)?;
                capture.step = Step::Settling(SETTLE_FRAMES);
            }
            RestartTarget::ObjectiveExpiry => {
                ensure!(
                    capture.before.map_name.eq_ignore_ascii_case("SOV02SMD.MAP"),
                    "objective expiry must use physical SOV02"
                );
                capture.step = Step::AwaitObjective;
            }
        }
        Ok(capture)
    }

    /// A physical left press and release on the sidebar's 0xF2 tab, through
    /// the in-game gadget list.
    fn click_briefing_tab(state: &mut AppState, route: &mut Vec<Value>, frame: u32) -> Result<()> {
        use crate::app::input::gadget_input;
        let view = crate::app::presentation::sidebar_render::current_sidebar_view(state)
            .context("no sidebar view")?;
        let rect = state
            .sidebar_frame()
            .rect_to_frame(view.top_buttons[0].rect);
        let point = [rect.x + rect.w / 2.0, rect.y + rect.h / 2.0];
        state.match_state.input.cursor_x = point[0];
        state.match_state.input.cursor_y = point[1];
        for pressed in [true, false] {
            gadget_input::handle_mouse_button_event(
                state,
                winit::event::MouseButton::Left,
                pressed,
            );
        }
        let presentation = &state.match_state.match_presentation;
        ensure!(
            presentation.in_game_menu == crate::ui::pause_menu::InGameMenuState::MissionBriefing,
            "the 0xF2 tab did not open the restate briefing"
        );
        // ALL01UMD.MAP's MISSIONMD Briefing key.
        let expected = crate::app::frontend::shell_pass::resolve_csf(state, "Brief:ALL01md");
        ensure!(
            presentation.restate_text == expected,
            "the restate briefing did not resolve Brief:ALL01md"
        );
        route.push(
            json!({"frame": frame, "action": "0xF2 tab press and release",
            "point": point, "text_chars": presentation.restate_text.chars().count()}),
        );
        Ok(())
    }

    /// A map's TAction 68 (Lose) through the House outcome owner; the
    /// SavourDelay, outcome EVA, defeat cascade and GameExit::Defeat tail
    /// then run unaided.
    #[cfg(feature = "dev-overlays")]
    fn complete_current_house(
        state: &mut AppState,
        route: &mut Vec<Value>,
        frame: u32,
        kind: crate::sim::house_state::HouseOutcomeKind,
    ) -> Result<()> {
        let runtime = state
            .match_state
            .sim_runtime
            .as_mut()
            .context("no live campaign runtime")?;
        ensure!(
            runtime
                .simulation
                .dev_trigger_current_house_outcome(&runtime.resources.rules, kind),
            "TAction did not flag the current House outcome"
        );
        route.push(
            json!({"frame": frame, "action": format!("TAction current House {kind:?}"),
            "tick": runtime.simulation.session().tick()}),
        );
        Ok(())
    }

    #[cfg(not(feature = "dev-overlays"))]
    fn complete_current_house(
        _: &mut AppState,
        _: &mut Vec<Value>,
        _: u32,
        _: crate::sim::house_state::HouseOutcomeKind,
    ) -> Result<()> {
        bail!("forcing a campaign defeat needs the dev-overlays build")
    }

    /// The restarted mission's first live frame holds tick zero for readback.
    pub(super) fn owns_simulation_steps(&self, state: &AppState) -> bool {
        matches!(self.step, Step::AwaitRestartedLive | Step::Done)
            && state.frontend.screen == GameScreen::InGame
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
            Step::AwaitObjective => {
                let expected = App::csf_label(state, "MISSION:Sov2Obj2", "MISSION:Sov2Obj2");
                let row = state
                    .match_state
                    .match_presentation
                    .message_list
                    .messages()
                    .iter()
                    .find(|row| row.text == expected);
                if let Some(row) = row {
                    let deadline_ms = row.deadline_ms.context("objective must have a deadline")?;
                    self.objective_pause = Some(ObjectivePause {
                        text: expected,
                        deadline_ms,
                        paused_tick: 0,
                    });
                    self.observe_objective(state, "objective visible");
                    self.abort = Some(abort_return::AbortReturnInput::begin_menu(
                        state, route, frame,
                    )?);
                    self.objective_pause.as_mut().unwrap().paused_tick =
                        Self::simulation_tick(state)?;
                    self.step = Step::AwaitObjectiveMenu;
                }
            }
            Step::AwaitObjectiveMenu => {
                if self
                    .abort
                    .as_mut()
                    .context("objective menu input owner")?
                    .after_present(state, route, frame)?
                {
                    self.observe_objective(state, "menu presented");
                    self.step = Step::HoldObjectiveMenu;
                }
            }
            Step::HoldObjectiveMenu => {
                let pause = self
                    .objective_pause
                    .as_ref()
                    .context("objective pause observation")?;
                ensure!(
                    Self::simulation_tick(state)? == pause.paused_tick,
                    "B5 advanced the campaign simulation"
                );
                let now = crate::app::input::tooltips::now_ms(state);
                if now > pause.deadline_ms + 64 {
                    ensure!(
                        state
                            .match_state
                            .match_presentation
                            .message_list
                            .messages()
                            .iter()
                            .any(|row| row.text == pause.text),
                        "Main_Tick expiry ran inside offline B5"
                    );
                    self.observe_objective(state, "menu held past deadline");
                    let layout = crate::app::input::pause_menu::current_layout(state)
                        .context("B5 layout")?;
                    let rect = layout
                        .button(crate::ui::shell::pause_menu::PauseMenuButton::Resume)
                        .context("B5 Resume")?;
                    let point = set_window_pointer_to_rect_center(state, rect);
                    crate::app::input::dispatch::handle_cursor_moved_in_game(state);
                    for edge in [
                        winit::event::ElementState::Pressed,
                        winit::event::ElementState::Released,
                    ] {
                        crate::app::input::dispatch::handle_mouse_input(
                            state,
                            winit::event::MouseButton::Left,
                            edge,
                        );
                    }
                    ensure!(
                        !state.match_state.match_presentation.in_game_menu.is_open(),
                        "Resume did not close B5"
                    );
                    route.push(json!({"frame": frame, "action": "Resume press and release", "point": point}));
                    self.step = Step::ObjectiveResumed;
                }
            }
            Step::ObjectiveResumed => {
                let pause = self
                    .objective_pause
                    .as_ref()
                    .context("objective pause observation")?;
                ensure!(
                    !state
                        .match_state
                        .match_presentation
                        .message_list
                        .messages()
                        .iter()
                        .any(|row| row.text == pause.text),
                    "objective survived the first resumed Main_Tick past its deadline"
                );
                self.observe_objective(state, "first resumed frame");
                Self::restore_pointer(state);
                self.step = Step::Done;
            }
            Step::AwaitBattleTheme => {
                let theme = state.audio.theme.current_song();
                let playing = state.audio.music_player.as_ref().is_some_and(|player| {
                    player.state() == crate::audio::theme::MusicOutputState::Playing
                });
                if playing
                    && usize::try_from(theme)
                        .ok()
                        .and_then(|index| state.audio.theme.entries().get(index))
                        .is_some_and(|entry| entry.normal)
                {
                    self.battle_theme = Some(theme);
                    self.observe_theme(state, "before defeat");
                    Self::complete_current_house(
                        state,
                        route,
                        frame,
                        crate::sim::house_state::HouseOutcomeKind::Defeat,
                    )?;
                    self.step = Step::AwaitDefeatPrompt;
                }
            }
            Step::AwaitVictoryScore => {
                if rendered == PresentedShell::Score
                    && App::campaign_score_shell_active(state)
                    && state
                        .frontend
                        .campaign_score_page
                        .as_ref()
                        .is_some_and(|page| page.presentation_started())
                {
                    crate::app::diagnostics::shell_capture::score::click_campaign_continue(state)?;
                    route.push(json!({"frame": frame, "action": "saved victory score Continue", "dialog": 0x103}));
                    self.step = Step::AwaitRestartedLive;
                }
            }
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
                    self.check_retained_battle_theme(state)?;
                    self.observe_theme(state, "replay prompt entered");
                    self.step = Step::HoldDefeatPrompt(SETTLE_FRAMES);
                }
            }
            Step::HoldDefeatPrompt(remaining) => {
                self.check_retained_battle_theme(state)?;
                if remaining > 0 {
                    self.step = Step::HoldDefeatPrompt(remaining - 1);
                } else {
                    self.observe_theme(state, "replay prompt serviced");
                    if self.target == RestartTarget::DefeatPrompt {
                        Self::restore_pointer(state);
                        self.step = Step::Settling(0);
                    } else if let Some((key, repeat)) = self.target.return_key() {
                        // Exercise the exact owner called by WindowEvent's
                        // physical-key branch, including ignored key edges.
                        ensure!(
                            !crate::app::frontend::shell_transition::blocks_shell_input(state)
                                && state.frontend.keyboard_dialog.is_none()
                                && state.frontend.fullscreen_movie.is_none()
                                && state.frontend.credits_roll.is_none(),
                            "an earlier WindowEvent owner would block the replay key"
                        );
                        for (ignored, pressed, repeated) in [
                            (key, false, false),
                            (winit::keyboard::KeyCode::Tab, true, false),
                            (winit::keyboard::KeyCode::Tab, true, true),
                        ] {
                            ensure!(App::handle_main_menu_dialog_key(
                                state, ignored, pressed, repeated
                            ));
                            ensure!(
                                state.frontend.shell_message_box.is_some(),
                                "release/unrelated key answered the replay box"
                            );
                        }
                        let modifiers = state.platform.live_modifiers;
                        state.platform.live_modifiers = winit::keyboard::ModifiersState::ALT;
                        for modified in [
                            winit::keyboard::KeyCode::Enter,
                            winit::keyboard::KeyCode::NumpadEnter,
                        ] {
                            for repeated in [false, true] {
                                ensure!(App::handle_main_menu_dialog_key(
                                    state, modified, true, repeated
                                ));
                                ensure!(
                                    state.frontend.shell_message_box.is_some(),
                                    "Alt+{modified:?} answered the replay box"
                                );
                            }
                        }
                        state.platform.live_modifiers = modifiers;
                        ensure!(App::handle_main_menu_dialog_key(state, key, true, repeat));
                        ensure!(
                            state.frontend.shell_message_box.is_none(),
                            "{key:?} press (repeat={repeat}) did not answer the replay box"
                        );
                        route.push(json!({"frame": frame, "action": "replay keyboard answer",
                            "physical_key": format!("{key:?}"), "repeat": repeat, "dialog": 0x120}));
                        Self::restore_pointer(state);
                        self.step = Step::AwaitShellReturn;
                    } else {
                        let answer_no = self.target == RestartTarget::DefeatReturned;
                        let layout = crate::ui::shell::modal::quit_confirm_layout(
                            state.renderer.gpu.config.width as i32,
                            state.renderer.gpu.config.height as i32,
                        );
                        let point = set_window_pointer_to_rect_center(
                            state,
                            if answer_no { layout.cancel } else { layout.ok },
                        );
                        App::handle_shell_message_box_mouse_down(state);
                        ensure!(
                            state.frontend.shell_controller.pressed()
                                == Some(if answer_no {
                                    crate::ui::shell::modal::control::CANCEL
                                } else {
                                    crate::ui::shell::modal::control::OK
                                }),
                            "physical replay answer press missed its loaded control"
                        );
                        route.push(
                            json!({"frame": frame, "action": if answer_no { "No press" } else { "Yes press" }, "dialog": 0x120,
                            "point": point}),
                        );
                        self.step = Step::ReplayPressed;
                    }
                }
            }
            Step::ReplayPressed => {
                self.check_retained_battle_theme(state)?;
                App::handle_shell_message_box_mouse_up(state);
                ensure!(
                    state.frontend.shell_message_box.is_none(),
                    "physical replay answer release did not answer the box"
                );
                let answer_no = self.target == RestartTarget::DefeatReturned;
                route.push(json!({"frame": frame, "action": if answer_no { "No release" } else { "Yes release" }, "dialog": 0x120}));
                Self::restore_pointer(state);
                self.step = if answer_no {
                    Step::AwaitShellReturn
                } else {
                    Step::AwaitRestartedLive
                };
            }
            Step::AwaitShellReturn => {
                if state.frontend.screen == GameScreen::MainMenu
                    && state.frontend.shell_message_box.is_none()
                    && state.audio.theme.current_song() == state.audio.theme.from_name("INTRO")
                    && steady_main_menu_capture_ready(MainMenuCaptureSnapshot::from_state(state))?
                {
                    ensure!(
                        state
                            .audio
                            .music_player
                            .as_ref()
                            .is_some_and(|player| player.state()
                                == crate::audio::theme::MusicOutputState::Playing),
                        "the returned shell's INTRO is not physically playing"
                    );
                    self.observe_theme(state, "shell after No");
                    route.push(json!({"frame": frame, "action": "shell resumed after No"}));
                    self.step = Step::Done;
                }
            }
            Step::AwaitRestartedLive => {
                if let Some(startup) = crate::app::loading::pump::loading_campaign_startup(state)
                    && self.observed_restart_loading.is_none()
                {
                    ensure!(
                        startup.plays_action_movie()
                            == (self.target == RestartTarget::SavedVictory),
                        "the continuation passed the wrong Start_Scenario movie flag"
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
                        after.campaign_index == self.before.campaign_index
                            && if self.target == RestartTarget::SavedVictory {
                                after.map_name == "ALL02UMD.MAP"
                                    && after.mission_counter == self.before.mission_counter + 1
                            } else {
                                after.map_name.eq_ignore_ascii_case(&self.before.map_name)
                                    && after.mission_counter == self.before.mission_counter
                            },
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
                    self.observe_theme(state, "restarted first live frame");
                    self.step = Step::Done;
                }
            }
        }
        Ok(())
    }

    fn restore_pointer(state: &mut AppState) {
        MoviesCapture::restore_neutral_pointer(state);
    }

    fn simulation_tick(state: &AppState) -> Result<u64> {
        Ok(state
            .match_state
            .sim_runtime
            .as_ref()
            .context("live campaign runtime")?
            .simulation
            .session()
            .tick())
    }

    fn observe_objective(&mut self, state: &AppState, phase: &str) {
        self.objective_observations.push(json!({"phase": phase,
            "wall_ms": crate::app::input::tooltips::now_ms(state), "tick": Self::simulation_tick(state).ok(),
            "rows": state.match_state.match_presentation.message_list.messages().iter()
                .map(|row| json!({"text": row.text, "deadline_ms": row.deadline_ms})).collect::<Vec<_>>() }));
    }

    fn check_retained_battle_theme(&self, state: &AppState) -> Result<()> {
        ensure!(
            Some(state.audio.theme.current_song()) == self.battle_theme,
            "the replay prompt replaced the retained battlefield Theme"
        );
        ensure!(
            state.audio.music_player.as_ref().is_some_and(
                |player| player.state() == crate::audio::theme::MusicOutputState::Playing
            ),
            "the replay prompt stopped physical battlefield music"
        );
        Ok(())
    }

    fn observe_theme(&mut self, state: &AppState, phase: &str) {
        let current = state.audio.theme.current_song();
        let entry = usize::try_from(current)
            .ok()
            .and_then(|index| state.audio.theme.entries().get(index));
        self.theme_observations.push(json!({"phase": phase, "current": current,
            "key": entry.map(|entry| &entry.key), "sound": entry.map(|entry| &entry.sound),
            "saved_slots": state.audio.theme.saved_slots(),
            "physical": state.audio.music_player.as_ref().map(|player| format!("{:?}", player.state()))}));
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
            RestartTarget::DefeatReturned
            | RestartTarget::DefeatReturnedEnter
            | RestartTarget::DefeatReturnedNumpadEnter
            | RestartTarget::DefeatReturnedEscape
            | RestartTarget::DefeatReturnedRepeatedEnter => {
                self.step == Step::Done && state.frontend.screen == GameScreen::MainMenu
            }
            RestartTarget::Restate => {
                self.step == Step::Settling(0)
                    && state.match_state.match_presentation.in_game_menu
                        == crate::ui::pause_menu::InGameMenuState::MissionBriefing
            }
            RestartTarget::AbortRestarted
            | RestartTarget::DefeatReplayed
            | RestartTarget::SavedAbortRestarted
            | RestartTarget::MenuDefeatReplayed
            | RestartTarget::SavedVictory => {
                self.step == Step::Done && state.frontend.screen == GameScreen::InGame
            }
            RestartTarget::ObjectiveExpiry => {
                self.step == Step::Done && state.frontend.screen == GameScreen::InGame
            }
        })
    }

    pub(super) fn receipt(&self) -> Value {
        json!({"target": format!("{:?}", self.target), "before": self.before.receipt(),
            "restart_loading": self.observed_restart_loading, "restored": self.restored,
            "after": self.after.as_ref().map(Mission::receipt), "theme_observations": self.theme_observations,
            "objective_observations": self.objective_observations})
    }
}
