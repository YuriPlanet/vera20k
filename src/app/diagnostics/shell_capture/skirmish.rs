//! Production-only Skirmish capture route. No shell state or renderer is cloned.

use super::*;
use crate::app::App;
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PresentedShell {
    Other,
    MainMenu,
    SinglePlayer,
    MoviesAndCredits,
    MovieList,
    FullscreenMovie,
    CreditsRoll,
    Skirmish,
    Campaign,
    LoadSavedGame,
    Options,
    WolWelcome,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Phase {
    #[default]
    MainMenu,
    SinglePlayer,
    Skirmish,
    /// Back pressed: the teardown slide runs toward the held tick.
    SlideOut,
    /// Choose Map pressed: `0x102` slides out and `0x6B` slides in.
    Chooser,
    /// Cancel pressed on `0x6B`: it slides out and `0x102` slides in again.
    ChooserReturn,
    /// Start Game pressed: `0x102` slides out and the scenario loads.
    Starting,
    /// The game's Leave was confirmed: the abort exit runs and the shell
    /// resumes.
    Quitting,
}

/// What a Start Game checkpoint captures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LoadingTarget {
    /// The black frame that replaces the closed shell.
    Blank,
    /// The loading screen's first frame; the map load then runs to the end.
    FirstFrame,
    /// The game runs [`QUIT_AFTER_FRAMES`] frames, then Leave through the
    /// in-game abort; the new `0x102` settled after its entry slide.
    AfterQuit,
}

/// In-game frames before the quit route presses Leave.
const QUIT_AFTER_FRAMES: u32 = 30;

/// What a Choose Map checkpoint captures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ChooserTarget {
    /// `0x6B` settled after its entry slide.
    Steady,
    /// `0x6B`'s entry slide held at a tick.
    Entry(u32),
    /// Cancel on `0x6B`, then `0x102` settled after its new entry slide.
    Return,
    /// Use Map on the list's first map while AI rows would not fit it: the
    /// eject box over the empty backdrop.
    Eject,
}

#[derive(Default)]
pub(super) struct SkirmishCapture {
    phase: Phase,
    route: Vec<Value>,
    selected_scene: Option<Value>,
    last_presented: Option<PresentedShell>,
    /// Capture Back's teardown slide held at this tick instead of the steady
    /// dialog.
    slide_out_tick: Option<u32>,
    /// Capture the entry slide held at this tick instead of the steady dialog.
    entry_tick: Option<u32>,
    entry_seen: bool,
    entry_held: bool,
    chooser: Option<ChooserTarget>,
    /// The steady chooser got the production mouse move at the resting
    /// pointer, as the retail helper's pointer rests over the map list.
    pointer_rested: bool,
    loading: Option<LoadingTarget>,
    in_game_frames: u32,
}

#[derive(Clone, Copy)]
struct CaptureGuard {
    main_menu_screen: bool,
    surface: (u32, u32),
    failed: bool,
    developer_shortcut: bool,
    software_cursor: bool,
    cursor: (f32, f32),
    interaction_active: bool,
}

impl CaptureGuard {
    fn validate(self) -> Result<()> {
        ensure!(self.main_menu_screen, "skirmish capture left the shell");
        ensure!(
            self.surface == (EXPECTED_WIDTH, EXPECTED_HEIGHT),
            "skirmish capture surface changed"
        );
        ensure!(
            !self.failed && !self.developer_shortcut,
            "skirmish capture encountered fallback or a developer shortcut"
        );
        ensure!(self.software_cursor, "software cursor unavailable");
        ensure!(
            self.cursor == (EXPECTED_CURSOR_X as f32, EXPECTED_CURSOR_Y as f32),
            "capture cursor moved"
        );
        ensure!(
            !self.interaction_active,
            "skirmish capture encountered modal, editing or interaction state"
        );
        Ok(())
    }
}

fn guard(state: &AppState, chooser: Option<ChooserTarget>, loading: bool) -> Result<()> {
    let shell = &state.frontend.skirmish_shell_state;
    CaptureGuard {
        main_menu_screen: state.frontend.screen == GameScreen::MainMenu
            || (loading
                && matches!(
                    state.frontend.screen,
                    GameScreen::Loading | GameScreen::InGame
                )),
        surface: (state.render_width(), state.render_height()),
        failed: state.frontend.main_menu_shell_failed,
        developer_shortcut: state.frontend.dev_skirmish_shell_enabled,
        software_cursor: state.use_software_cursor(),
        // The game moves the pointer; the quit route puts it back at rest
        // when the shell returns (and its readiness requires that).
        cursor: if loading && state.frontend.screen != GameScreen::Loading {
            (EXPECTED_CURSOR_X as f32, EXPECTED_CURSOR_Y as f32)
        } else {
            (
                state.match_state.input.cursor_x,
                state.match_state.input.cursor_y,
            )
        },
        interaction_active: state.main_menu_dialog_open()
            || state.frontend.quit_cascade.is_some()
            || state.match_state.match_presentation.show_save_load_panel
            || (shell.choose_map_modal.is_some() && chooser.is_none())
            || (chooser != Some(ChooserTarget::Eject)
                && shell
                    .choose_map_modal
                    .as_ref()
                    .is_some_and(|modal| modal.eject_prompt.is_some()))
            || shell.validation_modal.is_some()
            || shell.random_map_setup_modal.is_some()
            || shell.saved_seed_browser.is_some()
            || state.frontend.random_map_generation.is_some()
            || shell.open_combo_dropdown.is_some()
            || shell.trackbar_drag.is_some()
            || shell.dropdown_scroll_drag.is_some()
            || shell.dropdown_scroll_press.is_some()
            || shell.pressed_owner_draw_button.is_some()
            || shell.player_name_edit.focused,
    }
    .validate()
}

fn selected_scene(state: &AppState) -> Result<Value> {
    let shell = &state.frontend.skirmish_shell_state;
    let map = state
        .frontend
        .scenario_catalog
        .shell_maps()
        .get(shell.selected_map_idx)
        .context("skirmish capture has no selected map")?;
    let preview = state
        .frontend
        .skirmish_preview_texture
        .as_ref()
        .context("selected map preview is unavailable for this capture checkpoint")?;
    ensure!(
        preview.selected_map_idx == shell.selected_map_idx
            && preview.setup_preview_generation.is_none(),
        "capture preview belongs to another selection"
    );
    Ok(json!({
        "map_file": map.file_name, "map_label": map.display_name,
        "map_capacity": map.player_capacity, "mode_id": shell.selected_mode_id,
        "preview": { "width": preview.width, "height": preview.height },
        "parsed_map_sha256": crate::util::sha256::sha256_hex(format!("{map:?}").as_bytes()),
        "parsed_map_hash_encoding": "Rust Debug representation; diagnostic identity only",
        "player": {
            "name": shell.player_name_edit.text, "country": format!("{:?}", shell.player_country),
            "country_random": shell.player_country_random, "color": shell.player_color_index,
            "color_claimed": shell.player_color_claimed,
            "start": format!("{:?}", shell.player_start_position), "team": shell.player_team
        },
        "opponents": shell.opponents.iter().enumerate().map(|(index, row)| json!({
            "visible": crate::ui::skirmish_shell::player_row_visible(shell, state.frontend.scenario_catalog.shell_maps(), index + 1),
            "enabled": row.enabled, "type": format!("{:?}", row.row_type),
            "country": format!("{:?}", row.country), "country_random": row.country_random,
            "color": row.color_index, "color_claimed": row.color_claimed,
            "start": format!("{:?}", row.start_position), "team": row.team
        })).collect::<Vec<_>>(),
        "options": {
            "credits": shell.starting_credits, "speed": shell.game_speed, "units": shell.unit_count,
            "short_game": shell.short_game, "super_weapons": shell.super_weapons,
            "build_off_ally": shell.build_off_ally, "crates": shell.crates,
            "mcv_redeploy": shell.mcv_redeploy, "zoom": shell.zoom_enabled
        }
    }))
}

/// The tick of `kind`'s entry slide, while it runs.
fn entry_wave_tick(state: &AppState, kind: ShellSlideKind) -> Option<u32> {
    if state.frontend.shell_slide_active_shell != Some(kind) {
        return None;
    }
    state
        .frontend
        .shell_first_paint_slide
        .as_ref()
        .and_then(|wave| wave.compatibility_tick())
}

impl SkirmishCapture {
    pub(super) fn slide_out(tick: u32) -> Self {
        Self {
            slide_out_tick: Some(tick),
            ..Self::default()
        }
    }

    pub(super) fn entry(tick: u32) -> Self {
        Self {
            entry_tick: Some(tick),
            ..Self::default()
        }
    }

    pub(super) fn chooser(target: ChooserTarget) -> Self {
        Self {
            chooser: Some(target),
            ..Self::default()
        }
    }

    pub(super) fn loading(target: LoadingTarget) -> Self {
        Self {
            loading: Some(target),
            ..Self::default()
        }
    }

    fn guard(&self, state: &AppState) -> Result<()> {
        guard(
            state,
            self.chooser,
            matches!(self.phase, Phase::Starting | Phase::Quitting),
        )
    }

    /// A press and release at `point` through the chooser's production
    /// handlers.
    fn click_chooser(state: &mut AppState, point: (i32, i32)) {
        state.match_state.input.cursor_x = point.0 as f32;
        state.match_state.input.cursor_y = point.1 as f32;
        App::handle_choose_map_modal_mouse_down(state);
        App::handle_choose_map_modal_mouse_up(state);
    }

    /// Track press above the map list's thumb (scrolls to the top), a press
    /// on the first row, then Use Map; the pointer returns to rest.
    fn press_use_map_on_first_map(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let layout = crate::ui::skirmish_shell::compute_choose_map_modal_layout(
            EXPECTED_WIDTH,
            EXPECTED_HEIGHT,
        );
        let map_list = |state: &AppState| {
            state
                .frontend
                .skirmish_shell_state
                .choose_map_modal
                .as_ref()
                .map(|modal| modal.map_geometry(&layout))
                .context("chooser closed")
        };
        let bar = map_list(state)?
            .scrollbar
            .context("the map list needs a scrollbar")?;
        Self::click_chooser(state, (bar.x + 10, bar.y + 24));
        let row = map_list(state)?.row(0);
        Self::click_chooser(state, (row.x + 4, row.y + 4));
        let button = layout.use_map_button;
        Self::click_chooser(state, (button.x + button.w / 2, button.y + button.h / 2));
        state.match_state.input.cursor_x = EXPECTED_CURSOR_X as f32;
        state.match_state.input.cursor_y = EXPECTED_CURSOR_Y as f32;
        ensure!(
            state
                .frontend
                .skirmish_shell_state
                .choose_map_modal
                .as_ref()
                .is_some_and(|modal| modal.eject_prompt.is_some()),
            "Use Map on the first map did not ask to eject AI players"
        );
        self.route.push(json!({"dialog": 0x6b, "frame": frame,
            "action": "Use Map on the first map", "row": row.y}));
        Ok(())
    }

    /// The route's time budget: the quit route loads and plays a game.
    pub(super) fn timeout(&self) -> std::time::Duration {
        if self.loading == Some(LoadingTarget::AfterQuit) {
            std::time::Duration::from_secs(180)
        } else {
            std::time::Duration::from_secs(60)
        }
    }

    /// `0x6B` shows with no slide and its heading and status line revealed.
    fn chooser_settled(state: &AppState) -> bool {
        state
            .frontend
            .skirmish_shell_state
            .choose_map_modal
            .is_some()
            && state.frontend.shell_first_paint_slide.is_none()
            && state.frontend.shell_exit.is_none()
            && state.frontend.shell_slide_active_shell == Some(ShellSlideKind::ChooseMap)
            && state.frontend.shell_page_title.is_terminal()
            && state.frontend.shell_status_line.is_terminal()
    }

    /// Called only after an ordinary production frame has been presented. Route
    /// actions run here, never between frame acquisition and shell dispatch.
    pub(super) fn after_present(
        &mut self,
        state: &mut AppState,
        rendered: PresentedShell,
        frame: u32,
    ) -> Result<()> {
        if self.phase == Phase::Quitting && state.frontend.screen == GameScreen::MainMenu {
            // Back in the shell the pointer rests at the centre again.
            state.match_state.input.cursor_x = EXPECTED_CURSOR_X as f32;
            state.match_state.input.cursor_y = EXPECTED_CURSOR_Y as f32;
        }
        self.guard(state)?;
        self.last_presented = Some(rendered);
        match (self.phase, rendered) {
            (Phase::MainMenu, PresentedShell::MainMenu) => {
                if steady_main_menu_capture_ready(MainMenuCaptureSnapshot::from_state(state))? {
                    self.route
                        .push(json!({"dialog": 0xe2, "frame": frame, "action": "SinglePlayer"}));
                    App::handle_main_menu_shell_action(
                        state,
                        crate::ui::main_menu_shell::MainMenuShellAction::SinglePlayer,
                    );
                    self.phase = Phase::SinglePlayer;
                }
            }
            (Phase::SinglePlayer, PresentedShell::SinglePlayer) => {
                ensure!(
                    state.frontend.shell_route.single_player(),
                    "Single Player route changed"
                );
                ensure!(
                    state
                        .frontend
                        .main_menu_movie_identity
                        .is_some_and(
                            |identity| identity.owner() == Ra2tsDialogOwner::SinglePlayer0x100
                        ),
                    "Single Player movie identity unavailable"
                );
                if state.frontend.shell_first_paint_slide.is_none()
                    && state.frontend.shell_slide_active_shell == Some(ShellSlideKind::SinglePlayer)
                {
                    self.route
                        .push(json!({"dialog": 0x100, "frame": frame, "action": "Skirmish"}));
                    App::handle_single_player_shell_action(
                        state,
                        crate::ui::single_player_shell::SinglePlayerShellAction::Skirmish,
                    );
                    self.phase = Phase::Skirmish;
                }
            }
            // The first-paint slide frames present through the generic slide
            // renderer, so they arrive as `Other`.
            (Phase::Skirmish, PresentedShell::Other | PresentedShell::Skirmish)
                if self.entry_tick.is_some() =>
            {
                let target = self.entry_tick.context("entry phase without a tick")?;
                match entry_wave_tick(state, ShellSlideKind::Skirmish) {
                    Some(tick) => {
                        self.entry_seen = true;
                        ensure!(tick <= target, "entry slide passed tick {target}");
                        if tick == target && !self.entry_held {
                            if let Some(wave) = state.frontend.shell_first_paint_slide.as_mut() {
                                wave.hold_for_capture();
                            }
                            self.entry_held = true;
                            self.route.push(json!({"dialog": 0x102, "frame": frame,
                                "action": "hold entry slide", "tick": tick}));
                        }
                    }
                    None => ensure!(
                        !self.entry_seen,
                        "Skirmish entry slide ended before capture"
                    ),
                }
            }
            (Phase::Skirmish, PresentedShell::Skirmish) => {
                if self.settled(state)? && self.selected_scene.is_none() {
                    self.selected_scene = Some(selected_scene(state)?);
                } else if self.selected_scene.is_some() && self.slide_out_tick.is_some() {
                    ensure!(
                        App::handle_skirmish_back(state)
                            == crate::app::shell_skirmish::SkirmishBackOutcome::Leaving
                            && state.frontend.shell_exit.is_some(),
                        "Skirmish Back did not start its teardown slide"
                    );
                    self.route
                        .push(json!({"dialog": 0x102, "frame": frame, "action": "Back"}));
                    self.phase = Phase::SlideOut;
                } else if self.selected_scene.is_some() && self.loading.is_some() {
                    // Start Game (`0x617`) through the production action.
                    App::start_game_from_shell(state);
                    ensure!(
                        state.frontend.shell_exit.is_some(),
                        "Start Game did not start 0x102's teardown slide"
                    );
                    self.route
                        .push(json!({"dialog": 0x102, "frame": frame, "action": "StartGame"}));
                    self.phase = Phase::Starting;
                } else if self.selected_scene.is_some() && self.chooser.is_some() {
                    App::leave_shell_dialog(
                        state,
                        crate::app::frontend::shell_transition::ShellExitThen::SkirmishChooseMap,
                    );
                    ensure!(
                        state.frontend.shell_exit.is_some(),
                        "Choose Map did not start 0x102's teardown slide"
                    );
                    self.route
                        .push(json!({"dialog": 0x102, "frame": frame, "action": "ChooseMap"}));
                    self.phase = Phase::Chooser;
                }
            }
            (Phase::Chooser, PresentedShell::Other | PresentedShell::Skirmish) => {
                match self.chooser {
                    Some(ChooserTarget::Entry(target)) => {
                        match entry_wave_tick(state, ShellSlideKind::ChooseMap) {
                            Some(tick) => {
                                self.entry_seen = true;
                                ensure!(tick <= target, "0x6B entry slide passed tick {target}");
                                if tick == target && !self.entry_held {
                                    if let Some(wave) =
                                        state.frontend.shell_first_paint_slide.as_mut()
                                    {
                                        wave.hold_for_capture();
                                    }
                                    self.entry_held = true;
                                    self.route.push(json!({"dialog": 0x6b, "frame": frame,
                                        "action": "hold entry slide", "tick": tick}));
                                }
                            }
                            None => ensure!(
                                !self.entry_seen
                                    || state.frontend.shell_slide_active_shell
                                        != Some(ShellSlideKind::ChooseMap),
                                "0x6B entry slide ended before capture"
                            ),
                        }
                    }
                    Some(ChooserTarget::Steady)
                        if !self.pointer_rested
                            && state.frontend.shell_first_paint_slide.is_none()
                            && state.frontend.shell_slide_active_shell
                                == Some(ShellSlideKind::ChooseMap) =>
                    {
                        App::handle_skirmish_shell_mouse_move(state);
                        self.pointer_rested = true;
                        self.route.push(json!({"dialog": 0x6b, "frame": frame,
                            "action": "pointer rests", "point": [
                                state.match_state.input.cursor_x,
                                state.match_state.input.cursor_y]}));
                    }
                    Some(ChooserTarget::Eject)
                        if Self::chooser_settled(state) && !self.pointer_rested =>
                    {
                        self.press_use_map_on_first_map(state, frame)?;
                        self.pointer_rested = true;
                    }
                    Some(ChooserTarget::Return) if Self::chooser_settled(state) => {
                        App::leave_shell_dialog(
                            state,
                            crate::app::frontend::shell_transition::ShellExitThen::ChooseMapCancel,
                        );
                        ensure!(
                            state.frontend.shell_exit.is_some(),
                            "Cancel did not start 0x6B's teardown slide"
                        );
                        self.route
                            .push(json!({"dialog": 0x6b, "frame": frame, "action": "Cancel"}));
                        self.phase = Phase::ChooserReturn;
                    }
                    _ => {}
                }
            }
            (Phase::Starting, _)
                if self.loading == Some(LoadingTarget::AfterQuit)
                    && state.frontend.screen == GameScreen::InGame =>
            {
                self.in_game_frames += 1;
                if self.in_game_frames == QUIT_AFTER_FRAMES {
                    // The abort box's Leave through the production action
                    // (queues the EXIT event, `0x004F192C`).
                    crate::app::input::abort::activate(
                        state,
                        crate::ui::shell::abort::AbortButton::Leave,
                    );
                    self.route.push(json!({"screen": "in game", "frame": frame,
                        "action": "Leave", "in_game_frames": self.in_game_frames}));
                    self.phase = Phase::Quitting;
                }
            }
            (Phase::SlideOut, PresentedShell::Skirmish) => {
                let target = self
                    .slide_out_tick
                    .context("slide-out phase without a tick")?;
                let tick = crate::app::frontend::shell_transition::shell_exit_wave(
                    state,
                    ShellSlideKind::Skirmish,
                )
                .context("Skirmish teardown ended before capture")?
                .compatibility_tick()
                .context("teardown slide without a tick clock")?;
                ensure!(tick <= target, "teardown slide passed tick {target}");
                if tick == target {
                    crate::app::frontend::shell_transition::hold_shell_exit_for_capture(state);
                    self.route.push(json!({"dialog": 0x102, "frame": frame,
                        "action": "hold teardown slide", "tick": tick}));
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn settled(&self, state: &AppState) -> Result<bool> {
        self.guard(state)?;
        ensure!(
            state
                .frontend
                .shell_route
                .skirmish_returns_to_single_player(),
            "skirmish capture bypassed Single Player"
        );
        ensure!(
            state.frontend.skirmish_shell_chrome.is_some(),
            "skirmish chrome unavailable"
        );
        ensure!(
            state.frontend.main_menu_movie.is_none(),
            "prior shell movie survived skirmish entry"
        );
        let shell = &state.frontend.skirmish_shell_state;
        Ok(state.frontend.shell_first_paint_slide.is_none()
            && state.frontend.shell_slide_active_shell == Some(ShellSlideKind::Skirmish)
            && shell.title_reveal.has_completed()
            && shell.game_type_reveal.has_completed()
            && shell.map_label_reveal.has_completed())
    }

    pub(super) fn ready(&self, state: &AppState) -> Result<bool> {
        self.guard(state)?;
        if let Some(target) = self.entry_tick {
            return Ok(
                self.entry_held && entry_wave_tick(state, ShellSlideKind::Skirmish) == Some(target)
            );
        }
        match self.chooser {
            Some(ChooserTarget::Steady) => {
                return Ok(self.phase == Phase::Chooser
                    && self.pointer_rested
                    && Self::chooser_settled(state));
            }
            Some(ChooserTarget::Eject) => {
                return Ok(self.phase == Phase::Chooser
                    && self.pointer_rested
                    && state
                        .frontend
                        .skirmish_shell_state
                        .choose_map_modal
                        .as_ref()
                        .is_some_and(|modal| modal.eject_prompt.is_some()));
            }
            Some(ChooserTarget::Entry(target)) => {
                return Ok(self.phase == Phase::Chooser
                    && self.entry_held
                    && entry_wave_tick(state, ShellSlideKind::ChooseMap) == Some(target));
            }
            Some(ChooserTarget::Return) => {
                return Ok(self.phase == Phase::ChooserReturn
                    && state
                        .frontend
                        .skirmish_shell_state
                        .choose_map_modal
                        .is_none()
                    && self.last_presented == Some(PresentedShell::Skirmish)
                    && self.settled(state)?);
            }
            None => {}
        }
        if self.loading == Some(LoadingTarget::AfterQuit) {
            return Ok(self.phase == Phase::Quitting
                && state.frontend.screen == GameScreen::MainMenu
                && (
                    state.match_state.input.cursor_x,
                    state.match_state.input.cursor_y,
                ) == (EXPECTED_CURSOR_X as f32, EXPECTED_CURSOR_Y as f32)
                && self.last_presented == Some(PresentedShell::Skirmish)
                && self.settled(state)?);
        }
        if let Some(target) = self.loading {
            let next = match target {
                LoadingTarget::Blank => crate::app::loading::pump::NextLoadingFrame::Blank,
                LoadingTarget::FirstFrame | LoadingTarget::AfterQuit => {
                    crate::app::loading::pump::NextLoadingFrame::First
                }
            };
            return Ok(self.phase == Phase::Starting
                && state.frontend.screen == GameScreen::Loading
                && crate::app::loading::pump::next_native_loading_frame(state) == Some(next));
        }
        if let Some(target) = self.slide_out_tick {
            return Ok(self.phase == Phase::SlideOut
                && self.last_presented == Some(PresentedShell::Skirmish)
                && crate::app::frontend::shell_transition::shell_exit_wave(
                    state,
                    ShellSlideKind::Skirmish,
                )
                .and_then(|wave| wave.compatibility_tick())
                    == Some(target));
        }
        if self.phase != Phase::Skirmish
            || self.last_presented != Some(PresentedShell::Skirmish)
            || self.selected_scene.is_none()
        {
            return Ok(false);
        }
        if !self.settled(state)? {
            return Ok(false);
        }
        ensure!(
            self.selected_scene.as_ref() == Some(&selected_scene(state)?),
            "selected skirmish state changed during capture"
        );
        Ok(true)
    }

    pub(super) fn manifest(
        &self,
        request: &ShellCaptureRequest,
        format: wgpu::TextureFormat,
        pixels: &[u8],
        frame: u32,
    ) -> Value {
        let mut manifest = json!({
            "schema_version": "vera20k.skirmish-shell-capture.v1", "checkpoint": request.checkpoint.as_str(),
            "parity_certification": "NONE", "presenter_domain": "final-swapchain-after-rgb565",
            "surface": {"width": request.width, "height": request.height, "format": format!("{format:?}"),
                "pixel_layout": "BGRA8", "row_order": "top-left", "row_stride": request.width * 4},
            "cursor": {"x": request.cursor_x, "y": request.cursor_y, "policy": "software-composited"},
            "route": self.route,
            "dialog_resource_id": match (self.chooser, self.loading) {
                (Some(ChooserTarget::Steady | ChooserTarget::Entry(_) | ChooserTarget::Eject), _) => {
                    Some(0x6b)
                }
                (_, Some(_)) => None,
                _ => Some(0x102),
            },
            "capture_frame": frame,
            "selection": self.selected_scene, "reveals_completed": self.entry_tick.is_none(),
            "ordinary_skirmish_frame": true,
            "input_enrollment": "UNENROLLED; asset/profile bytes require enrollment before native comparison",
            "frame": {"path": FRAME_FILE_NAME, "byte_length": pixels.len(),
                "sha256": crate::util::sha256::sha256_hex(pixels)}
        });
        // Only loading checkpoints carry the key; the validator
        // (`tools/shell_certification/skirmish.py`) pins the others' key set.
        if let Some(target) = self.loading {
            manifest["loading_frame"] = json!(format!("{target:?}"));
        }
        manifest
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_guard_rejects_each_invalid_boundary() {
        let valid = CaptureGuard {
            main_menu_screen: true,
            surface: (800, 600),
            failed: false,
            developer_shortcut: false,
            software_cursor: true,
            cursor: (400.0, 300.0),
            interaction_active: false,
        };
        assert!(valid.validate().is_ok());
        for invalid in [
            CaptureGuard {
                main_menu_screen: false,
                ..valid
            },
            CaptureGuard {
                surface: (640, 480),
                ..valid
            },
            CaptureGuard {
                failed: true,
                ..valid
            },
            CaptureGuard {
                developer_shortcut: true,
                ..valid
            },
            CaptureGuard {
                software_cursor: false,
                ..valid
            },
            CaptureGuard {
                cursor: (401.0, 300.0),
                ..valid
            },
            CaptureGuard {
                cursor: (400.0, f32::NAN),
                ..valid
            },
            CaptureGuard {
                interaction_active: true,
                ..valid
            },
        ] {
            assert!(invalid.validate().is_err());
        }
    }
}
