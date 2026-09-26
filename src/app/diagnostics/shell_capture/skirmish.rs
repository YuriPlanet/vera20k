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
    Score,
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
    /// Create Random Map pressed: `0x6B` slides out and `0x105` runs.
    RandomMap,
    /// Cancel pressed on `0x105`: it slides out and `0x6B` slides in again.
    RandomMapCancel,
    /// The hidden chooser's eject box after `0x105`'s Use Map was answered.
    RandomMapEject,
    /// Start Game pressed: `0x102` slides out and the scenario loads.
    Starting,
    /// The game's Leave was confirmed: the abort exit runs and the shell
    /// resumes.
    Quitting,
    /// In the game: the MCV deploys, then the Construction Yard is sold.
    Defeating,
    /// The score page `0x108` is up after the defeat.
    Scoring,
    /// Continue was pressed on the settled score page.
    Continuing,
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
    /// A real defeat: the local MCV deploys and its Construction Yard is sold
    /// through the production commands; the score page settled.
    DefeatScore,
    /// The same, then Continue: the new `0x102` settled.
    DefeatContinue,
}

/// In-game frames before the quit route presses Leave.
const QUIT_AFTER_FRAMES: u32 = 30;

/// Start Game's centre at 800x600 (the retail helper's `sk-hover-start.png`).
const START_GAME_POINT: (i32, i32) = (720, 262);

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
    /// Create Random Map, then the random-map dialog `0x105`.
    RandomMap(RandomMapTarget),
}

/// What a random-map dialog checkpoint captures. The route hovers Create
/// Random Map before pressing it, as the retail helper's click does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RandomMapTarget {
    /// `0x105` settled after its entry slide.
    Steady,
    /// `0x105`'s entry slide held at a tick.
    Entry(u32),
    /// `0x105` settled with the pointer resting here.
    Hover((i32, i32)),
    /// Cancel: `0x105` slides out and `0x6B`'s entry slide is held at a tick.
    CancelEntry(u32),
    /// Cancel: `0x6B` settled again.
    CancelSteady,
    /// Use Map: the map generates, `0x105` slides out, the chooser's Use Map
    /// selects it and `0x102` settles with the random map.
    UseMap,
    /// Generate Map, then Save Map opens the seed browser over `0x105`; its
    /// Back uncovers `0x105`, which must not slide in again.
    SeedBrowserReturn,
    /// Terrain Type's list open, `0x105` settled with the pointer below the
    /// list on Map Size's face. The list holds the mouse, so the status line
    /// keeps Terrain Type's help.
    ListOpen,
    /// AI row 2 takes Easy on `0x102` and Players 2 on `0x105`, then Generate
    /// Map and Use Map: `0x105` slides out and the hidden chooser's Use Map
    /// asks to eject AI players while no dialog shows or slides. Cancel slides
    /// the chooser in and it settles; OK closes it without a slide and `0x102`
    /// settles with the random map.
    Eject(crate::ui::skirmish_shell::EjectPromptButton),
    /// The same route's eject box, presented over no dialog.
    EjectBox,
}

/// Create Random Map on `0x6B` and Cancel on `0x105` at 800x600.
const CREATE_RANDOM_MAP_POINT: (i32, i32) = (720, 262);
const RANDOM_MAP_CANCEL_POINT: (i32, i32) = (720, 556);
const RANDOM_MAP_USE_MAP_POINT: (i32, i32) = (720, 220);
const RANDOM_MAP_GENERATE_POINT: (i32, i32) = (432, 430);
/// Terrain Type's arrow, and Map Size's face below its open list, at 800x600
/// (the retail helper's `ej-list-open.png` click and `ej-list-off.png` point).
const RANDOM_MAP_TERRAIN_ARROW_POINT: (i32, i32) = (484, 79);
const RANDOM_MAP_MAP_SIZE_FACE_POINT: (i32, i32) = (350, 196);

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
    /// Once `0x102` settles, rest the pointer here through the production
    /// mouse move.
    hover: Option<(i32, i32)>,
    hovered: bool,
    /// The random-map routes rest the pointer on Create Random Map until the
    /// chooser's help has typed in before pressing it: the retail helper's
    /// click rests 100 ms before its press and 90 ms more before its release,
    /// and `rmg-cancel.png` shows that help whole.
    create_random_map_hovered: bool,
    /// The random-map Use Map or seed-browser route has pressed Generate Map.
    generate_pressed: bool,
    /// The seed-browser route has pressed Save Map.
    save_pressed: bool,
    /// The eject route has set AI row 2, then Players 2, and has seen its
    /// box with no dialog behind it.
    second_ai_added: bool,
    players_set: bool,
    eject_seen: bool,
    /// The dialog the eject box's answer shows got the production mouse move
    /// at the resting pointer once its slide had run.
    answer_rested: bool,
    loading: Option<LoadingTarget>,
    in_game_frames: u32,
    deploy_sent: bool,
    sell_sent: bool,
}

#[derive(Clone, Copy)]
struct CaptureGuard {
    main_menu_screen: bool,
    surface: (u32, u32),
    failed: bool,
    developer_shortcut: bool,
    software_cursor: bool,
    cursor: (f32, f32),
    expected_cursor: (f32, f32),
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
        ensure!(self.cursor == self.expected_cursor, "capture cursor moved");
        ensure!(
            !self.interaction_active,
            "skirmish capture encountered modal, editing or interaction state"
        );
        Ok(())
    }
}

fn guard(
    state: &AppState,
    chooser: Option<ChooserTarget>,
    loading: bool,
    expected_cursor: (f32, f32),
) -> Result<()> {
    let shell = &state.frontend.skirmish_shell_state;
    CaptureGuard {
        main_menu_screen: state.frontend.screen == GameScreen::MainMenu
            || (loading
                && matches!(
                    state.frontend.screen,
                    GameScreen::Loading | GameScreen::InGame | GameScreen::MissionResult { .. }
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
        expected_cursor,
        interaction_active: state.main_menu_dialog_open()
            || state.frontend.quit_cascade.is_some()
            || state.match_state.match_presentation.show_save_load_panel
            || (shell.choose_map_modal.is_some() && chooser.is_none())
            || (shell.random_map_setup_modal.is_some()
                && !matches!(chooser, Some(ChooserTarget::RandomMap(_))))
            || (!matches!(
                chooser,
                Some(
                    ChooserTarget::Eject
                        | ChooserTarget::RandomMap(
                            RandomMapTarget::Eject(_) | RandomMapTarget::EjectBox
                        )
                )
            ) && shell
                .choose_map_modal
                .as_ref()
                .is_some_and(|modal| modal.eject_prompt.is_some()))
            || shell.validation_modal.is_some()
            || (shell.saved_seed_browser.is_some()
                && chooser != Some(ChooserTarget::RandomMap(RandomMapTarget::SeedBrowserReturn)))
            || (state.frontend.random_map_generation.is_some()
                && !matches!(
                    chooser,
                    Some(ChooserTarget::RandomMap(
                        RandomMapTarget::UseMap
                            | RandomMapTarget::SeedBrowserReturn
                            | RandomMapTarget::Eject(_)
                            | RandomMapTarget::EjectBox
                    ))
                ))
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

    pub(super) fn hover(point: (i32, i32)) -> Self {
        Self {
            hover: Some(point),
            ..Self::default()
        }
    }

    pub(super) fn random_map(target: RandomMapTarget) -> Self {
        Self::chooser(ChooserTarget::RandomMap(target))
    }

    /// Where the pointer rests: on the hovered control once the route has
    /// moved it there, else at the capture's rest point.
    fn rest_point(&self) -> (i32, i32) {
        let hover = match self.chooser {
            Some(ChooserTarget::RandomMap(RandomMapTarget::Hover(point))) => Some(point),
            Some(ChooserTarget::RandomMap(RandomMapTarget::ListOpen)) => {
                Some(RANDOM_MAP_MAP_SIZE_FACE_POINT)
            }
            _ => self.hover,
        };
        hover
            .filter(|_| self.hovered)
            .unwrap_or((EXPECTED_CURSOR_X as i32, EXPECTED_CURSOR_Y as i32))
    }

    fn set_pointer(state: &mut AppState, point: (i32, i32)) {
        state.match_state.input.cursor_x = point.0 as f32;
        state.match_state.input.cursor_y = point.1 as f32;
    }

    fn guard(&self, state: &AppState) -> Result<()> {
        let rest = self.rest_point();
        guard(
            state,
            self.chooser,
            matches!(
                self.phase,
                Phase::Starting
                    | Phase::Quitting
                    | Phase::Defeating
                    | Phase::Scoring
                    | Phase::Continuing
            ),
            (rest.0 as f32, rest.1 as f32),
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

    fn defeat_route(&self) -> bool {
        matches!(
            self.loading,
            Some(LoadingTarget::DefeatScore | LoadingTarget::DefeatContinue)
        )
    }

    /// The local player's first entity (stable order) whose rules type
    /// matches.
    fn local_entity(
        state: &AppState,
        wanted: impl Fn(&crate::rules::object_type::ObjectType) -> bool,
    ) -> Option<u64> {
        let owner = crate::app::input::commands::preferred_local_owner_name(state)?;
        let sim = &state.match_state.sim_runtime.as_ref()?.simulation;
        let rules = state.rules()?;
        sim.entities()
            .values_sorted()
            .filter(|entity| sim.interner.resolve(entity.owner()) == owner)
            .find(|entity| {
                rules
                    .object(sim.interner.resolve(entity.type_ref()))
                    .is_some_and(&wanted)
            })
            .map(|entity| entity.stable_id())
    }

    /// Deploy the MCV, then sell the Construction Yard it becomes: with Short
    /// Game on the house has no base left and loses. The score page follows
    /// through the production exit cascade.
    fn lose_the_game(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        if matches!(state.frontend.screen, GameScreen::MissionResult { .. }) {
            ensure!(
                App::score_shell_active(state),
                "the defeat reached a result screen without the score page"
            );
            let page = state
                .frontend
                .score_page
                .as_ref()
                .context("score page missing")?;
            let rows: Vec<Value> = page
                .model
                .rows
                .iter()
                .map(|row| {
                    json!({"name": row.name, "rgb": row.rgb, "kills": row.kills,
                        "losses": row.losses, "built": row.built, "score": row.score})
                })
                .collect();
            self.route.push(
                json!({"dialog": 0x108, "frame": frame, "action": "score page",
                "title_key": page.model.title_key, "game": page.model.game_number,
                "seconds": page.model.elapsed_seconds, "side": page.model.side, "rows": rows}),
            );
            self.phase = Phase::Scoring;
            return Ok(());
        }
        let owner = crate::app::input::commands::preferred_local_owner_name(state)
            .context("no local owner in the game")?;
        if !self.deploy_sent {
            let mcv = Self::local_entity(state, |object| object.deploys_into.is_some())
                .context("the local player has no MCV")?;
            crate::app::input::commands::schedule_command(
                state,
                &owner,
                crate::sim::command::Command::DeployMcv { entity_id: mcv },
            );
            self.route.push(json!({"screen": "in game", "frame": frame,
                "action": "deploy MCV", "entity": mcv}));
            self.deploy_sent = true;
        } else if !self.sell_sent
            && let Some(yard) = Self::local_entity(state, |object| object.construction_yard)
        {
            crate::app::input::commands::schedule_command(
                state,
                &owner,
                crate::sim::command::Command::SellBuilding { entity_id: yard },
            );
            self.route.push(json!({"screen": "in game", "frame": frame,
                "action": "sell Construction Yard", "entity": yard}));
            self.sell_sent = true;
        }
        Ok(())
    }

    /// The score page shows with no slide and every reveal finished.
    fn score_settled(state: &AppState) -> bool {
        state.frontend.shell_first_paint_slide.is_none()
            && state.frontend.shell_exit.is_none()
            && state.frontend.shell_page_title.is_terminal()
            && state.frontend.shell_status_line.is_terminal()
            && state
                .frontend
                .score_page
                .as_ref()
                .is_some_and(|page| page.reveals_terminal())
    }

    /// The route's time budget: the quit and defeat routes load and play a
    /// game.
    pub(super) fn timeout(&self) -> std::time::Duration {
        if matches!(
            self.loading,
            Some(
                LoadingTarget::AfterQuit
                    | LoadingTarget::DefeatScore
                    | LoadingTarget::DefeatContinue
            )
        ) {
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
            && state
                .frontend
                .skirmish_shell_state
                .choose_map_modal
                .as_ref()
                .is_some_and(|modal| modal.statics.is_terminal())
    }

    /// `0x105` shows with no slide and its heading and status line revealed.
    fn random_map_settled(state: &AppState) -> bool {
        state.frontend.shell_first_paint_slide.is_none()
            && state.frontend.shell_exit.is_none()
            && state.frontend.shell_slide_active_shell == Some(ShellSlideKind::RandomMap)
            && state
                .frontend
                .skirmish_shell_state
                .random_map_setup_modal
                .as_ref()
                .is_some_and(|setup| setup.statics.is_terminal())
    }

    /// Generate Map, Save Map once the map exists, then the browser's Back
    /// (`0x686`); from then on `0x105` shows without an entry slide.
    fn seed_browser_step(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let shell = &state.frontend.skirmish_shell_state;
        let browsing = shell.saved_seed_browser.is_some();
        if self.pointer_rested {
            ensure!(!browsing, "the seed browser stayed open after Back");
            ensure!(
                state.frontend.shell_first_paint_slide.is_none(),
                "closing the seed browser replayed 0x105's entry slide"
            );
            return Ok(());
        }
        let centre =
            |rect: crate::ui::skirmish_shell::RectPx| (rect.x + rect.w / 2, rect.y + rect.h / 2);
        // The covered `0x105` paints nothing, so its statics wait.
        if browsing {
            let back = crate::ui::skirmish_shell::compute_saved_seed_layout(
                crate::ui::skirmish_shell::SavedSeedMode::Save,
                state.render_width(),
                state.render_height(),
            )
            .back;
            Self::set_pointer(state, centre(back));
            App::handle_saved_seed_browser_mouse_down(state);
            App::handle_saved_seed_browser_mouse_up(state);
            Self::set_pointer(state, self.rest_point());
            self.pointer_rested = true;
            self.route
                .push(json!({"dialog": 0x2b4, "frame": frame, "action": "Back"}));
            return Ok(());
        }
        if !Self::random_map_settled(state) {
            return Ok(());
        }
        let generated = shell
            .random_map_setup_modal
            .as_ref()
            .is_some_and(|setup| setup.generated && !setup.generating);
        let press = if !self.generate_pressed {
            self.generate_pressed = true;
            Some((RANDOM_MAP_GENERATE_POINT, "GenerateMap"))
        } else if generated && !self.save_pressed {
            self.save_pressed = true;
            let save = crate::ui::skirmish_shell::compute_random_map_setup_layout(
                state.render_width(),
                state.render_height(),
            )
            .save;
            Some((centre(save), "SaveMap"))
        } else {
            None
        };
        if let Some((point, action)) = press {
            Self::set_pointer(state, point);
            App::handle_skirmish_shell_mouse_move(state);
            App::handle_random_map_setup_mouse_down(state);
            App::handle_random_map_setup_mouse_up(state);
            Self::set_pointer(state, self.rest_point());
            self.route
                .push(json!({"dialog": 0x105, "frame": frame, "action": action}));
            if action == "SaveMap" {
                ensure!(
                    state
                        .frontend
                        .skirmish_shell_state
                        .saved_seed_browser
                        .is_some(),
                    "Save Map did not open the seed browser"
                );
            }
        }
        Ok(())
    }

    /// Hold `kind`'s entry slide once it reaches `target`.
    /// Generate Map, then Use Map once the map exists (Use Map starts
    /// disabled, as retail's `rmg-steady.png`). With `min_players` the
    /// route first presses the Players track's left end, as the retail
    /// helper does until the value reaches its minimum, 2 (`ej-players.png`).
    fn use_map_step(&mut self, state: &mut AppState, frame: u32, min_players: bool) {
        let generated = state
            .frontend
            .skirmish_shell_state
            .random_map_setup_modal
            .as_ref()
            .is_some_and(|setup| setup.generated && !setup.generating);
        let press = if min_players && !self.players_set {
            self.players_set = true;
            let track = crate::ui::skirmish_shell::compute_random_map_setup_layout(
                state.render_width(),
                state.render_height(),
            )
            .control_rects[5];
            Some(((track.x + 3, track.y + track.h / 2), "Players 2"))
        } else if !self.generate_pressed {
            self.generate_pressed = true;
            Some((RANDOM_MAP_GENERATE_POINT, "GenerateMap"))
        } else if generated && !self.pointer_rested {
            self.pointer_rested = true;
            Some((RANDOM_MAP_USE_MAP_POINT, "UseMap"))
        } else {
            None
        };
        if let Some((point, action)) = press {
            Self::set_pointer(state, point);
            App::handle_skirmish_shell_mouse_move(state);
            App::handle_random_map_setup_mouse_down(state);
            App::handle_random_map_setup_mouse_up(state);
            Self::set_pointer(state, self.rest_point());
            self.route
                .push(json!({"dialog": 0x105, "frame": frame, "action": action}));
        }
    }

    /// AI row 2 takes Easy through its type combo's production presses: the
    /// arrow opens the list and its second row picks Easy (retail
    /// `ej-ai-open.png`, `ej-ai-easy.png`).
    fn add_second_ai(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        use crate::ui::skirmish_shell::{SkirmishAiRowType, SkirmishComboId, SkirmishComboItem};
        let id = SkirmishComboId::AiType(1);
        let layout =
            crate::ui::skirmish_shell::compute_layout(state.render_width(), state.render_height());
        let face = crate::ui::skirmish_shell::combo_rect(&layout, id)
            .context("AI row 2 has no type combo")?;
        Self::set_pointer(
            state,
            (
                face.x + face.w - crate::ui::skirmish_shell::COMBO_ARROW_RESERVE_W / 2,
                face.y + crate::ui::skirmish_shell::COMBO_FACE_H / 2,
            ),
        );
        App::handle_skirmish_shell_mouse_down(state);
        let shell = &state.frontend.skirmish_shell_state;
        let maps = state.frontend.scenario_catalog.shell_maps();
        let list = crate::ui::skirmish_shell::combo_dropdown_content_rect(shell, &layout, maps, id)
            .filter(|_| shell.open_combo_dropdown.is_some())
            .context("AI row 2's type list did not open")?;
        let row = crate::ui::skirmish_shell::combo_items(shell, maps, id)
            .iter()
            .position(|item| *item == SkirmishComboItem::AiType(SkirmishAiRowType::Easy))
            .context("the AI type list has no Easy row")? as i32;
        let row_h = crate::ui::skirmish_shell::COMBO_DROPDOWN_ROW_H;
        Self::set_pointer(
            state,
            (list.x + list.w / 2, list.y + row * row_h + row_h / 2),
        );
        App::handle_skirmish_shell_mouse_down(state);
        crate::ui::skirmish_shell::handle_option_mouse_up(&mut state.frontend.skirmish_shell_state);
        Self::set_pointer(state, self.rest_point());
        ensure!(
            state
                .frontend
                .skirmish_shell_state
                .opponents
                .get(1)
                .is_some_and(|row| row.row_type == SkirmishAiRowType::Easy),
            "AI row 2 did not take Easy"
        );
        self.second_ai_added = true;
        self.selected_scene = Some(selected_scene(state)?);
        self.route
            .push(json!({"dialog": 0x102, "frame": frame, "action": "AI row 2 Easy"}));
        Ok(())
    }

    /// Players 2, Generate Map and Use Map; once `0x105` has slid out, the
    /// hidden chooser's eject box asks while no dialog shows or slides, and
    /// the route answers it a presented frame later (`None` leaves it up).
    fn eject_step(
        &mut self,
        state: &mut AppState,
        answer: Option<crate::ui::skirmish_shell::EjectPromptButton>,
        frame: u32,
    ) -> Result<()> {
        let shell = &state.frontend.skirmish_shell_state;
        // Until Use Map is pressed (`pointer_rested`) and `0x105` has gone.
        if !self.pointer_rested || shell.random_map_setup_modal.is_some() {
            if Self::random_map_settled(state) {
                self.use_map_step(state, frame, true);
                if let Some(setup) = state
                    .frontend
                    .skirmish_shell_state
                    .random_map_setup_modal
                    .as_ref()
                {
                    ensure!(setup.options.num_players == 2, "Players did not reach 2");
                }
            }
            return Ok(());
        }
        ensure!(
            shell
                .choose_map_modal
                .as_ref()
                .is_some_and(|modal| modal.eject_prompt.is_some()),
            "Use Map on the two-player random map did not ask to eject AI players"
        );
        ensure!(
            shell.top_dialog().is_none()
                && state.frontend.shell_first_paint_slide.is_none()
                && state.frontend.shell_exit.is_none(),
            "a dialog shows or slides behind the eject box"
        );
        if !self.eject_seen {
            self.eject_seen = true;
            self.route.push(json!({"dialog": 0x6b, "frame": frame,
                "action": "eject box over no dialog"}));
            return Ok(());
        }
        let Some(answer) = answer else {
            return Ok(());
        };
        let layout = crate::ui::shell::modal::quit_confirm_layout(
            state.render_width() as i32,
            state.render_height() as i32,
        );
        let button = match answer {
            crate::ui::skirmish_shell::EjectPromptButton::Ok => layout.ok,
            crate::ui::skirmish_shell::EjectPromptButton::Cancel => layout.cancel,
        };
        Self::set_pointer(state, (button.x + button.w / 2, button.y + button.h / 2));
        App::handle_choose_map_eject_mouse_down(state);
        App::handle_choose_map_eject_mouse_up(state);
        Self::set_pointer(state, self.rest_point());
        let shell = &state.frontend.skirmish_shell_state;
        match answer {
            crate::ui::skirmish_shell::EjectPromptButton::Ok => ensure!(
                shell.choose_map_modal.is_none() && state.frontend.shell_exit.is_none(),
                "OK on the eject box slid the hidden chooser out"
            ),
            crate::ui::skirmish_shell::EjectPromptButton::Cancel => ensure!(
                shell.top_dialog()
                    == Some(crate::ui::skirmish_shell::SkirmishShellDialog::ChooseMap),
                "Cancel on the eject box did not show the chooser"
            ),
        }
        self.route.push(json!({"dialog": 0x6b, "frame": frame,
            "action": format!("eject box {answer:?}")}));
        self.phase = Phase::RandomMapEject;
        Ok(())
    }

    fn hold_entry(
        &mut self,
        state: &mut AppState,
        kind: ShellSlideKind,
        target: u32,
        dialog: u16,
        frame: u32,
    ) -> Result<()> {
        match entry_wave_tick(state, kind) {
            Some(tick) => {
                self.entry_seen = true;
                ensure!(
                    tick <= target,
                    "{dialog:#x} entry slide passed tick {target}"
                );
                if tick == target && !self.entry_held {
                    if let Some(wave) = state.frontend.shell_first_paint_slide.as_mut() {
                        wave.hold_for_capture();
                    }
                    self.entry_held = true;
                    self.route.push(json!({"dialog": dialog, "frame": frame,
                        "action": "hold entry slide", "tick": tick}));
                }
            }
            None => ensure!(
                !self.entry_seen || state.frontend.shell_slide_active_shell != Some(kind),
                "{dialog:#x} entry slide ended before capture"
            ),
        }
        Ok(())
    }

    /// Called only after an ordinary production frame has been presented. Route
    /// actions run here, never between frame acquisition and shell dispatch.
    pub(super) fn after_present(
        &mut self,
        state: &mut AppState,
        rendered: PresentedShell,
        frame: u32,
    ) -> Result<()> {
        if matches!(self.phase, Phase::Quitting | Phase::Continuing)
            && state.frontend.screen == GameScreen::MainMenu
        {
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
                } else if let Some(point) = self
                    .hover
                    .filter(|_| self.selected_scene.is_some() && !self.hovered)
                {
                    state.match_state.input.cursor_x = point.0 as f32;
                    state.match_state.input.cursor_y = point.1 as f32;
                    App::handle_skirmish_shell_mouse_move(state);
                    self.hovered = true;
                    self.route.push(json!({"dialog": 0x102, "frame": frame,
                        "action": "pointer rests", "point": [point.0, point.1]}));
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
                    // The pointer moves onto Start Game (`0x617`), whose help
                    // reaches the status line as it would before a click,
                    // then the production action runs and the pointer rests
                    // again.
                    state.match_state.input.cursor_x = START_GAME_POINT.0 as f32;
                    state.match_state.input.cursor_y = START_GAME_POINT.1 as f32;
                    App::handle_skirmish_shell_mouse_move(state);
                    App::start_game_from_shell(state);
                    state.match_state.input.cursor_x = EXPECTED_CURSOR_X as f32;
                    state.match_state.input.cursor_y = EXPECTED_CURSOR_Y as f32;
                    ensure!(
                        state.frontend.shell_exit.is_some(),
                        "Start Game did not start 0x102's teardown slide"
                    );
                    self.route.push(json!({"dialog": 0x102, "frame": frame,
                        "action": "StartGame", "hover": [START_GAME_POINT.0, START_GAME_POINT.1]}));
                    self.phase = Phase::Starting;
                } else if self.selected_scene.is_some()
                    && matches!(
                        self.chooser,
                        Some(ChooserTarget::RandomMap(
                            RandomMapTarget::Eject(_) | RandomMapTarget::EjectBox
                        ))
                    )
                    && !self.second_ai_added
                {
                    self.add_second_ai(state, frame)?;
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
                    Some(ChooserTarget::RandomMap(_))
                        if Self::chooser_settled(state) && !self.create_random_map_hovered =>
                    {
                        Self::set_pointer(state, CREATE_RANDOM_MAP_POINT);
                        App::handle_skirmish_shell_mouse_move(state);
                        Self::set_pointer(state, self.rest_point());
                        self.create_random_map_hovered = true;
                    }
                    Some(ChooserTarget::RandomMap(_)) if Self::chooser_settled(state) => {
                        // Through the chooser's production press and release.
                        Self::click_chooser(state, CREATE_RANDOM_MAP_POINT);
                        Self::set_pointer(state, self.rest_point());
                        ensure!(
                            state.frontend.shell_exit.is_some(),
                            "Create Random Map did not start 0x6B's teardown slide"
                        );
                        self.route.push(json!({"dialog": 0x6b, "frame": frame,
                            "action": "CreateRandomMap", "hover": [
                                CREATE_RANDOM_MAP_POINT.0, CREATE_RANDOM_MAP_POINT.1]}));
                        self.phase = Phase::RandomMap;
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
            (Phase::RandomMap, PresentedShell::Other | PresentedShell::Skirmish) => {
                let Some(ChooserTarget::RandomMap(target)) = self.chooser else {
                    bail!("the random-map phase runs only for random-map targets");
                };
                match target {
                    RandomMapTarget::Entry(target) => {
                        self.hold_entry(state, ShellSlideKind::RandomMap, target, 0x105, frame)?;
                    }
                    RandomMapTarget::Hover(point)
                        if Self::random_map_settled(state) && !self.hovered =>
                    {
                        Self::set_pointer(state, point);
                        App::handle_skirmish_shell_mouse_move(state);
                        self.hovered = true;
                        self.route.push(json!({"dialog": 0x105, "frame": frame,
                            "action": "pointer rests", "point": [point.0, point.1]}));
                    }
                    RandomMapTarget::ListOpen
                        if Self::random_map_settled(state) && !self.hovered =>
                    {
                        // Open the list through the dialog's production
                        // handlers, then rest the pointer below it.
                        Self::set_pointer(state, RANDOM_MAP_TERRAIN_ARROW_POINT);
                        App::handle_skirmish_shell_mouse_move(state);
                        App::handle_random_map_setup_mouse_down(state);
                        App::handle_random_map_setup_mouse_up(state);
                        ensure!(
                            state
                                .frontend
                                .skirmish_shell_state
                                .random_map_setup_modal
                                .as_ref()
                                .is_some_and(|setup| setup.open_combo.is_some()),
                            "Terrain Type's list did not open"
                        );
                        Self::set_pointer(state, RANDOM_MAP_MAP_SIZE_FACE_POINT);
                        App::handle_skirmish_shell_mouse_move(state);
                        self.hovered = true;
                        self.route.push(json!({"dialog": 0x105, "frame": frame,
                            "action": "open Terrain Type, pointer rests", "point": [
                                RANDOM_MAP_MAP_SIZE_FACE_POINT.0,
                                RANDOM_MAP_MAP_SIZE_FACE_POINT.1]}));
                    }
                    RandomMapTarget::CancelEntry(_) | RandomMapTarget::CancelSteady
                        if Self::random_map_settled(state) =>
                    {
                        // Cancel through the dialog's production handlers.
                        Self::set_pointer(state, RANDOM_MAP_CANCEL_POINT);
                        App::handle_skirmish_shell_mouse_move(state);
                        App::handle_random_map_setup_mouse_down(state);
                        App::handle_random_map_setup_mouse_up(state);
                        Self::set_pointer(state, self.rest_point());
                        ensure!(
                            state.frontend.shell_exit.is_some(),
                            "Cancel did not start 0x105's teardown slide"
                        );
                        self.route
                            .push(json!({"dialog": 0x105, "frame": frame, "action": "Cancel"}));
                        self.phase = Phase::RandomMapCancel;
                    }
                    RandomMapTarget::UseMap if Self::random_map_settled(state) => {
                        self.use_map_step(state, frame, false);
                    }
                    RandomMapTarget::SeedBrowserReturn => self.seed_browser_step(state, frame)?,
                    RandomMapTarget::Eject(answer) => {
                        self.eject_step(state, Some(answer), frame)?
                    }
                    RandomMapTarget::EjectBox => self.eject_step(state, None, frame)?,
                    _ => {}
                }
            }
            (Phase::RandomMapCancel, PresentedShell::Other | PresentedShell::Skirmish) => {
                match self.chooser {
                    Some(ChooserTarget::RandomMap(RandomMapTarget::CancelEntry(target)))
                        if state.frontend.shell_exit.is_none() =>
                    {
                        self.hold_entry(state, ShellSlideKind::ChooseMap, target, 0x6b, frame)?;
                    }
                    // The retail helper's pointer goes back to rest after its
                    // click; the queued move reaches the chooser once its
                    // slide has run (`rmg-cancel-b.png`).
                    Some(ChooserTarget::RandomMap(RandomMapTarget::CancelSteady))
                        if !self.pointer_rested
                            && state.frontend.shell_exit.is_none()
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
                    _ => {}
                }
            }
            (Phase::RandomMapEject, PresentedShell::Other | PresentedShell::Skirmish) => {
                let Some(ChooserTarget::RandomMap(RandomMapTarget::Eject(answer))) = self.chooser
                else {
                    bail!("the eject phase runs only for eject targets");
                };
                ensure!(
                    crate::app::frontend::shell_transition::shell_exit_wave(
                        state,
                        ShellSlideKind::ChooseMap
                    )
                    .is_none(),
                    "the chooser slid out after the eject box"
                );
                // Cancel shows the chooser, whose first paint slides it in
                // (`0x005E6B47`); OK shows `0x102` again.
                let kind = match answer {
                    crate::ui::skirmish_shell::EjectPromptButton::Cancel => {
                        ShellSlideKind::ChooseMap
                    }
                    crate::ui::skirmish_shell::EjectPromptButton::Ok => ShellSlideKind::Skirmish,
                };
                if entry_wave_tick(state, kind).is_some() {
                    self.entry_seen = true;
                }
                // The retail helper's pointer goes back to rest after its
                // click; the queued move reaches the dialog once its slide has
                // run.
                if self.entry_seen
                    && !self.answer_rested
                    && state.frontend.shell_first_paint_slide.is_none()
                    && state.frontend.shell_slide_active_shell == Some(kind)
                {
                    App::handle_skirmish_shell_mouse_move(state);
                    self.answer_rested = true;
                    let dialog = if kind == ShellSlideKind::ChooseMap {
                        0x6b
                    } else {
                        0x102
                    };
                    self.route.push(json!({"dialog": dialog, "frame": frame,
                        "action": "pointer rests", "point": [
                            state.match_state.input.cursor_x,
                            state.match_state.input.cursor_y]}));
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
            (Phase::Starting, _)
                if self.defeat_route() && state.frontend.screen == GameScreen::InGame =>
            {
                self.in_game_frames += 1;
                if self.in_game_frames == QUIT_AFTER_FRAMES {
                    self.phase = Phase::Defeating;
                }
            }
            (Phase::Defeating, _) => self.lose_the_game(state, frame)?,
            (Phase::Scoring, PresentedShell::Score) if Self::score_settled(state) => {
                if self.loading == Some(LoadingTarget::DefeatContinue) {
                    // Continue through the page's production handlers.
                    state.match_state.input.cursor_x = 720.0;
                    state.match_state.input.cursor_y = 556.0;
                    App::handle_score_shell_mouse_move(state);
                    App::handle_score_shell_mouse_down(state);
                    App::handle_score_shell_mouse_up(state);
                    state.match_state.input.cursor_x = EXPECTED_CURSOR_X as f32;
                    state.match_state.input.cursor_y = EXPECTED_CURSOR_Y as f32;
                    ensure!(
                        state.frontend.shell_exit.is_some(),
                        "Continue did not start the 0x108 teardown slide"
                    );
                    self.route
                        .push(json!({"dialog": 0x108, "frame": frame, "action": "Continue"}));
                    self.phase = Phase::Continuing;
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
        Ok(state.frontend.shell_first_paint_slide.is_none()
            && state.frontend.shell_slide_active_shell == Some(ShellSlideKind::Skirmish)
            && state.frontend.skirmish_shell_state.statics.is_terminal())
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
            Some(ChooserTarget::RandomMap(target)) => {
                return Ok(match target {
                    RandomMapTarget::Steady => {
                        self.phase == Phase::RandomMap && Self::random_map_settled(state)
                    }
                    RandomMapTarget::Entry(tick) => {
                        self.phase == Phase::RandomMap
                            && self.entry_held
                            && entry_wave_tick(state, ShellSlideKind::RandomMap) == Some(tick)
                    }
                    RandomMapTarget::Hover(_) | RandomMapTarget::ListOpen => {
                        self.phase == Phase::RandomMap
                            && self.hovered
                            && Self::random_map_settled(state)
                    }
                    RandomMapTarget::CancelEntry(tick) => {
                        self.phase == Phase::RandomMapCancel
                            && self.entry_held
                            && entry_wave_tick(state, ShellSlideKind::ChooseMap) == Some(tick)
                    }
                    RandomMapTarget::UseMap => {
                        self.pointer_rested
                            && state.frontend.random_map_generation.is_none()
                            && state
                                .frontend
                                .skirmish_shell_state
                                .choose_map_modal
                                .is_none()
                            && self.last_presented == Some(PresentedShell::Skirmish)
                            && self.settled(state)?
                    }
                    RandomMapTarget::SeedBrowserReturn => {
                        self.phase == Phase::RandomMap
                            && self.pointer_rested
                            && state
                                .frontend
                                .skirmish_shell_state
                                .saved_seed_browser
                                .is_none()
                            && Self::random_map_settled(state)
                    }
                    RandomMapTarget::CancelSteady => {
                        self.phase == Phase::RandomMapCancel
                            && self.pointer_rested
                            && state
                                .frontend
                                .skirmish_shell_state
                                .random_map_setup_modal
                                .is_none()
                            && Self::chooser_settled(state)
                    }
                    RandomMapTarget::Eject(
                        crate::ui::skirmish_shell::EjectPromptButton::Cancel,
                    ) => {
                        self.phase == Phase::RandomMapEject
                            && self.answer_rested
                            && Self::chooser_settled(state)
                    }
                    RandomMapTarget::EjectBox => self.phase == Phase::RandomMap && self.eject_seen,
                    RandomMapTarget::Eject(crate::ui::skirmish_shell::EjectPromptButton::Ok) => {
                        self.phase == Phase::RandomMapEject
                            && self.answer_rested
                            && state
                                .frontend
                                .skirmish_shell_state
                                .choose_map_modal
                                .is_none()
                            && self.last_presented == Some(PresentedShell::Skirmish)
                            && self.settled(state)?
                    }
                });
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
        if self.loading == Some(LoadingTarget::DefeatScore) {
            return Ok(self.phase == Phase::Scoring
                && self.last_presented == Some(PresentedShell::Score)
                && App::score_shell_active(state)
                && Self::score_settled(state));
        }
        if self.loading == Some(LoadingTarget::DefeatContinue) {
            return Ok(self.phase == Phase::Continuing
                && state.frontend.screen == GameScreen::MainMenu
                && self.last_presented == Some(PresentedShell::Skirmish)
                && self.settled(state)?);
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
                LoadingTarget::FirstFrame
                | LoadingTarget::AfterQuit
                | LoadingTarget::DefeatScore
                | LoadingTarget::DefeatContinue => {
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
            || self.hover.is_some() != self.hovered
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
                (
                    Some(
                        ChooserTarget::Steady
                        | ChooserTarget::Entry(_)
                        | ChooserTarget::Eject
                        | ChooserTarget::RandomMap(
                            RandomMapTarget::CancelEntry(_)
                            | RandomMapTarget::CancelSteady
                            | RandomMapTarget::Eject(
                                crate::ui::skirmish_shell::EjectPromptButton::Cancel,
                            )
                            | RandomMapTarget::EjectBox,
                        ),
                    ),
                    _,
                ) => Some(0x6b),
                (
                    Some(ChooserTarget::RandomMap(
                        RandomMapTarget::UseMap
                        | RandomMapTarget::Eject(crate::ui::skirmish_shell::EjectPromptButton::Ok),
                    )),
                    _,
                ) => Some(0x102),
                (Some(ChooserTarget::RandomMap(_)), _) => Some(0x105),
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
            expected_cursor: (400.0, 300.0),
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
