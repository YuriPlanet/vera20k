//! In-game menu input. Native callback4F11B0 owns the commands of B5, BBA and
//! FF; rendering and hit-testing share the loaded side-art geometry
//! (72FC60,60B000,60B350,60B7A0).

use crate::app::match_runtime::session::{PauseEventLimiters, SessionMode, WallBucketTimer};
use crate::app::persistence::options_profile::RetailOptionsProfile;
use crate::app::{App, AppState};
use crate::ui::main_menu_dialogs::OptionsDialogState;
use crate::ui::main_menu_dialogs::options::shell::{
    IN_GAME_KEYBOARD_HELP, IN_GAME_LANDING, LauncherOptionsLayout, LauncherStatusHelp,
};
use crate::ui::main_menu_dialogs::options::{
    Category, LauncherOptionsEvent, LauncherOptionsPacked, LauncherTrackbarId,
};
use crate::ui::pause_menu::InGameMenuState;
use crate::ui::shell::geom::ShellScale;
use crate::ui::shell::pause_menu::{
    InternetSlider, PauseMenuButton, PauseMenuButtonState, PauseMenuLayout, PauseMenuTemplate,
    pause_menu_layout,
};
use crate::ui::shell::trackbar::{TrackbarHold, TrackbarPress};
use crate::ui::skirmish_shell::SavedSeedMode;
use winit::event::MouseButton;

/// FF's two trackbars as the dialog holds them between init and Resume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct InternetPauseControls {
    /// Trackbar values in [`InternetSlider`] order.
    pub(crate) positions: [i32; 2],
    pub(crate) hold: Option<TrackbarHold<InternetSlider>>,
    pub(crate) hovered: Option<InternetSlider>,
}

impl InternetPauseControls {
    /// Init4F1720: `TBM_SETRANGE` then `TBM_SETPOS` with `3 - LatencyFudge`
    /// and `6 - GameSpeed`.
    pub(crate) fn open(latency_fudge: i32, game_speed: i32) -> Self {
        Self {
            positions: [
                InternetSlider::Connection.range().set_up(3 - latency_fudge),
                InternetSlider::Speed.range().set_up(
                    crate::ui::shell::game_speed::SpeedContext::Network.visual_position(game_speed),
                ),
            ],
            ..Self::default()
        }
    }

    /// The right static's text: the CSF label for the current position.
    pub(crate) fn value_caption(&self, slider: InternetSlider) -> (&'static str, &'static str) {
        slider.value_caption(self.positions[slider as usize])
    }

    /// Store a pointer-selected position; a change is the trackbar's
    /// position notification (61E609..61E6DD).
    fn set(&mut self, slider: InternetSlider, position: i32) -> bool {
        let stored = &mut self.positions[slider as usize];
        if *stored == position {
            return false;
        }
        *stored = position;
        true
    }
}

/// VERA's Options pages on the in-game menu: the main menu's dialog, built
/// again from the profile each time the menu opens and committed when it
/// closes. Sound and Keyboard change some of the same values between those
/// moments, so neither copy outlives the other's turn.
///
/// The page the menu opens on is Game Controls: gamemd's `0xBBB` settings and
/// the three volumes of its Sound child, with the template's own Game
/// Controls button as its tab. gamemd opens BBB as a dialog of its own from
/// that button (4F11B0 writes state 5).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InGameOptionsPage {
    pub(crate) dialog: OptionsDialogState,
    /// What the controls showed at open: only one the player moves writes.
    opened: LauncherOptionsPacked,
    /// The profile at open, which the page's live changes write to.
    opened_profile: RetailOptionsProfile,
    /// Whether BBB's init shows the Game Speed row
    /// (`in_game_options::on_open`), and where its slider stood at open.
    game_speed_shown: bool,
    opened_speed_position: u8,
}

impl InGameOptionsPage {
    /// Nothing moved since the menu opened, so closing it leaves VERA.INI as
    /// it is.
    fn untouched(&self, profile: &RetailOptionsProfile) -> bool {
        self.dialog.pack() == self.opened && *profile == self.opened_profile
    }

    /// The Game Speed position the player moved the slider to, if they did.
    fn moved_speed_position(&self) -> Option<u8> {
        let position = self.dialog.trackbar_position(LauncherTrackbarId::GameSpeed);
        (self.game_speed_shown && position != self.opened_speed_position).then_some(position)
    }
}

/// Fit the match to changed display choices, as a window resize does: the
/// zoom pass, then the sidebar laid out again for the new frame.
fn refit_match(state: &mut AppState) {
    App::sync_zoom_pass(state);
    crate::app::presentation::sidebar_render::refresh_sidebar_projection(state);
}

/// The template the open in-game menu was created from.
pub(crate) fn current_template(state: &AppState) -> PauseMenuTemplate {
    state.match_state.match_presentation.pause_menu_template
}

/// `0x004F10E1..0x004F1130` for the running session.
fn select_template(state: &AppState) -> PauseMenuTemplate {
    let mode = state.match_state.session_mode();
    let local_defeated = state
        .match_state
        .sim_runtime
        .runtime()
        .is_some_and(|runtime| runtime.simulation.local_player_defeated());
    PauseMenuTemplate::select(
        mode.is_single_player(),
        mode == SessionMode::Wol,
        local_defeated,
    )
}

pub(crate) fn current_layout(state: &AppState) -> Option<PauseMenuLayout> {
    let shell = crate::app::frontend::skirmish_shell_render::current_in_game_shell_layout(state)?;
    let mut layout = pause_menu_layout(
        shell.width,
        shell.height,
        shell.layout,
        shell.button_size,
        current_template(state),
    );
    // The menu's Options pages have their buttons under Game Controls.
    layout.add_page_tabs();
    if multiplayer_control(state).is_some() {
        layout.enable_shared_pause();
    }
    // FF's group heads the Game Options page and has no place on the others.
    if let Some(page) = state.match_state.match_presentation.options_page.as_ref() {
        if page.dialog.category() == Category::Gameplay {
            layout.raise_internet_group();
        } else {
            layout.internet = None;
        }
    }
    Some(layout)
}

/// The Options page showing on the open in-game menu, in the shell's pixels.
pub(crate) fn options_page_layout(state: &AppState) -> Option<LauncherOptionsLayout> {
    let page = state.match_state.match_presentation.options_page.as_ref()?;
    let shell = crate::app::frontend::skirmish_shell_render::current_in_game_shell_layout(state)?;
    let battle = App::match_zoom_geometry(state);
    let menu = current_layout(state)?;
    Some(
        LauncherOptionsLayout::in_game(
            (shell.width, shell.height),
            menu.page_tabs(),
            page.dialog.category(),
            current_template(state) == PauseMenuTemplate::Internet,
            page.game_speed_shown,
            menu.button(PauseMenuButton::Keyboard).is_none(),
            (
                state.renderer.gpu.config.width,
                state.renderer.gpu.config.height,
            ),
        )
        .with_battle_frame((battle.src_width, battle.src_height)),
    )
}

/// What the page's controls did takes effect through the main menu's own
/// path. A match has to be fitted to a new frame as a window resize does.
fn run_options_page(state: &mut AppState) {
    let Some(page) = state.match_state.match_presentation.options_page.as_mut() else {
        return;
    };
    let output = page.dialog.drain_output();
    let language = output
        .events
        .iter()
        .any(|event| matches!(event, LauncherOptionsEvent::LanguageSelected(_)));
    let child = output.events.iter().find_map(|event| match event {
        LauncherOptionsEvent::InGameKeyboardRequested => Some(InGameMenuState::Keyboard),
        LauncherOptionsEvent::InGameSoundRequested => Some(InGameMenuState::Sound),
        _ => None,
    });
    let refit = output.events.iter().any(|event| {
        matches!(
            event,
            LauncherOptionsEvent::ZoomSelected(_)
                | LauncherOptionsEvent::SidebarSelected(_)
                | LauncherOptionsEvent::OriginalShapeChanged(_)
                | LauncherOptionsEvent::HdRadarChanged(_)
                | LauncherOptionsEvent::SmoothSidebarChanged(_)
        )
    });
    for event in output.events {
        crate::app::persistence::options::launcher::dispatch_launcher_preview_event(state, event);
    }
    if refit {
        refit_match(state);
    }
    if language {
        let labels = App::launcher_options_labels(state);
        if let Some(page) = state.match_state.match_presentation.options_page.as_mut() {
            page.dialog.set_labels(labels);
        }
    }
    // BBB's 0x52C and 0x52D: the dialogs open over the menu, which closes
    // and commits its page on the way, and come back to it.
    match child {
        Some(InGameMenuState::Keyboard) => crate::app::input::keyboard::open(
            state,
            crate::ui::shell::keyboard::KeyboardParent::GameControls,
        ),
        Some(InGameMenuState::Sound) => crate::app::input::sound::open(state),
        _ => {}
    }
}

/// The pointer over the Options page: `pressed` is the left button's new
/// state, `None` a move. True when the menu's other controls don't get to see
/// it: an open list or a held slider has the pointer, or the page has gone.
fn options_page_pointer(state: &mut AppState, pressed: Option<bool>) -> bool {
    let Some(layout) = options_page_layout(state) else {
        return false;
    };
    let (x, y) = crate::app::frontend::skirmish_shell_render::in_game_shell_cursor(state);
    let Some(page) = state.match_state.match_presentation.options_page.as_mut() else {
        return false;
    };
    let was_taken = page.dialog.shell_pointer_taken();
    match pressed {
        Some(true) => page.dialog.shell_mouse_down(x, y, &layout),
        Some(false) => page.dialog.shell_mouse_up(x, y, &layout),
        None => page.dialog.shell_mouse_move(x, y, &layout),
    }
    run_options_page(state);
    was_taken || state.match_state.match_presentation.options_page.is_none()
}

/// Esc closes the page's open list before anything else hears it.
pub(crate) fn close_options_list(state: &mut AppState) -> bool {
    let closed = state
        .match_state
        .match_presentation
        .options_page
        .as_mut()
        .is_some_and(|page| page.dialog.shell_close_list());
    if closed {
        run_options_page(state);
    }
    closed
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct MultiplayerPauseControl {
    /// [`crate::i18n`] labels, resolved where they are drawn.
    pub(crate) caption: &'static str,
    pub(crate) enabled: bool,
    pub(crate) status: Option<&'static str>,
}

fn multiplayer_control_for(
    pause: crate::net::live_commands::MultiplayerPauseStatus,
    stopped_here: bool,
) -> Option<MultiplayerPauseControl> {
    pause.available.then(|| {
        let (caption, status) = if pause.pending {
            if pause.paused {
                ("VERA:Resuming", Some("VERA:ResumeRequested"))
            } else {
                ("VERA:Pausing", Some("VERA:PauseRequested"))
            }
        } else if pause.paused && !stopped_here {
            ("VERA:Pausing", Some("VERA:FinishingPause"))
        } else if pause.paused {
            ("VERA:ResumeAllPlayers", Some("VERA:PausedForAllPlayers"))
        } else {
            ("VERA:PauseAllPlayers", None)
        };
        MultiplayerPauseControl {
            caption,
            enabled: !pause.pending && (!pause.paused || stopped_here),
            status,
        }
    })
}

pub(crate) fn multiplayer_control(state: &AppState) -> Option<MultiplayerPauseControl> {
    let pause = state
        .match_state
        .live_network
        .as_ref()?
        .multiplayer_pause()?;
    multiplayer_control_for(pause, state.match_state.multiplayer_paused())
}

/// The explicit relay action is serviced without simulation ticks. Repeats
/// while awaiting confirmation are consumed, never queued as toggles.
pub(crate) fn toggle_multiplayer_pause(state: &mut AppState) -> bool {
    let Some(control) = multiplayer_control(state) else {
        return false;
    };
    if control.enabled
        && let Some(pause) = live_pause(state)
    {
        request_multiplayer_pause(state, !pause.paused);
    }
    true
}

fn live_pause(state: &AppState) -> Option<crate::net::live_commands::MultiplayerPauseStatus> {
    state.match_state.live_network.as_ref()?.multiplayer_pause()
}

/// Asks the relay to pause or resume, unless the match is ending. True when
/// the request went out.
fn request_multiplayer_pause(state: &mut AppState, paused: bool) -> bool {
    let sent = state.match_state.scenario_exit.is_none()
        && state.match_state.scenario_outcome.is_none()
        && state
            .match_state
            .live_network
            .as_mut()
            .is_some_and(|live| live.request_multiplayer_pause(paused));
    if sent {
        state.platform.window.request_redraw();
    }
    sent
}

/// What a `/pause` (`want`) or `/resume` chat line does with the shared
/// pause `control`: whether to ask the relay, and the [`crate::i18n`] label
/// that answers the player. A match without the shared pause says so; a
/// pause that is already there or on its way shows its status instead.
fn chat_command_for(
    control: Option<MultiplayerPauseControl>,
    paused: bool,
    want: bool,
) -> (bool, Option<&'static str>) {
    match control {
        None => (false, Some("VERA:PauseNotAvailable")),
        Some(control) if !control.enabled || paused == want => (false, control.status),
        Some(_) if want => (true, Some("VERA:PauseRequested")),
        Some(_) => (true, Some("VERA:ResumeRequested")),
    }
}

/// VERA: `/pause` and `/resume` typed in match chat (`input::chat`) ask for
/// the Pause key's shared pause and answer with a message row.
pub(crate) fn chat_command(state: &mut AppState, want: bool) {
    let paused = live_pause(state).is_some_and(|pause| pause.paused);
    let (ask, answer) = chat_command_for(multiplayer_control(state), paused, want);
    if let Some(label) = answer
        && (!ask || request_multiplayer_pause(state, want))
    {
        crate::app::input::messages::post_chat_command_answer(state, &crate::i18n::tr(label));
    }
}

/// The applied GameSpeed FF's init (4F1720) reads; a queued speed event
/// changes it only when it executes.
fn current_game_speed(state: &AppState) -> i32 {
    state
        .match_state
        .sim_runtime
        .runtime()
        .map_or(0, |runtime| {
            runtime.simulation.session().game_options().game_speed
        })
}

/// Entering state 1 recreates the dialog: 4F10E0 picks its template and FF's
/// init reads the live values.
pub(crate) fn on_open(state: &mut AppState) {
    state.match_state.match_presentation.pause_menu_template = select_template(state);
    state.match_state.match_presentation.internet_pause = InternetPauseControls::open(
        state.match_state.session.latency_fudge(),
        current_game_speed(state),
    );
    // Escape opens on Game Controls, the settings a player most often reaches
    // for mid-match. Back from one of the menu's dialogs shows the page it
    // left (`App::enter_in_game_menu_state` forgets it on the way to play).
    let category = state
        .match_state
        .match_presentation
        .options_page_category
        .unwrap_or(IN_GAME_LANDING);
    // BBB's init (4E1FE0, message 0x497) decides the Game Speed row and
    // reads the speed in effect.
    crate::app::input::in_game_options::on_open(state);
    let options = &state.match_state.match_presentation.in_game_options;
    let (context, game_speed_shown) = (options.speed_context, !options.game_speed_hidden);
    let speed_position = u8::try_from(options.game_speed_position()).unwrap_or(0);
    let csf = state.process_assets.csf.as_ref();
    let label = |key: &str, fallback: &str| crate::i18n::label(csf, key, fallback);
    let speed_names = (0..=crate::ui::shell::game_speed::MAX_POSITION)
        .map(|position| label(context.label_key(context.stored_speed(position)), ""))
        .collect();
    let speed_caption = label("GUI:GameSpeed", "Game Speed");
    let sound_label = label("GUI:Sound", "Sound");
    let mut dialog = App::launcher_options_from_state(state);
    dialog.set_in_game_controls(speed_caption, speed_names, speed_position, sound_label);
    dialog.select_category(category);
    let opened = dialog.pack();
    let opened_profile = state.persistence.options_profile.clone();
    state.match_state.match_presentation.options_page = Some(InGameOptionsPage {
        opened_speed_position: dialog.trackbar_position(LauncherTrackbarId::GameSpeed),
        dialog,
        opened,
        opened_profile,
        game_speed_shown,
    });
}

/// The menu is closing: what its page holds applies and is saved before the
/// next page reads those values.
pub(crate) fn on_close(state: &mut AppState) {
    close_page(state, true);
}

/// The match is ending, or the program quitting, with the menu open. The
/// page's settings are still saved, but a moved Game Speed is dropped: only
/// BBB's Back result (1) applies and writes it through 4E1DE0, its result 2
/// does neither, and a speed command would reach a session that is over.
pub(crate) fn on_match_end(state: &mut AppState) {
    close_page(state, false);
}

fn close_page(state: &mut AppState, apply_speed: bool) {
    let Some(page) = state.match_state.match_presentation.options_page.take() else {
        return;
    };
    state.match_state.match_presentation.options_page_category = Some(page.dialog.category());
    // A moved Game Speed is BBB's accepted speed (4E1DE0): the profile and
    // the match take it, and the commit below writes the profile once.
    let speed = page.moved_speed_position().filter(|_| apply_speed);
    if let Some(position) = speed {
        let options = &mut state.match_state.match_presentation.in_game_options;
        let stored = options.speed_context.stored_speed(i32::from(position));
        options.game_speed = u32::try_from(stored).unwrap_or(0);
        state.persistence.options_profile.game_speed = stored;
        crate::app::persistence::options::apply_in_game_speed(state);
    }
    if speed.is_some() || !page.untouched(&state.persistence.options_profile) {
        App::commit_in_game_options_page(state, page.dialog, page.opened);
    }
}

pub(crate) fn button_states(state: &AppState) -> [PauseMenuButtonState; 8] {
    let interaction = state.match_state.match_presentation.pause_menu_interaction;
    // Native4F17D9..4F1832 applies559C20 eligibility to Load/Delete only, and
    // only in modes0/5; BBA/FF have neither control.
    let has_saves = state.match_state.match_presentation.pause_menu_has_saves;
    let controls_page = state
        .match_state
        .match_presentation
        .options_page
        .as_ref()
        .is_some_and(|page| page.dialog.category() == IN_GAME_LANDING);
    PauseMenuButton::ALL.map(|button| PauseMenuButtonState {
        pressed: interaction.is_pressed(button),
        // C5 timer highlighting is separate from pointer hover. VERA lights
        // Game Controls while its page shows, as a tab.
        highlighted: button == PauseMenuButton::GameControls && controls_page,
        enabled: if button == PauseMenuButton::SharedPause {
            multiplayer_control(state).is_some_and(|control| control.enabled)
        } else {
            has_saves || !matches!(button, PauseMenuButton::Load | PauseMenuButton::Delete)
        },
    })
}

/// The status-line help for the control under the pointer.
pub(crate) fn hovered_help(state: &AppState) -> Option<LauncherStatusHelp> {
    let template = current_template(state);
    let presentation = &state.match_state.match_presentation;
    if let Some(help) = presentation
        .options_page
        .as_ref()
        .and_then(|page| page.dialog.shell_status_help())
    {
        return Some(help);
    }
    if let Some(slider) = presentation.internet_pause.hovered {
        return Some(LauncherStatusHelp::Csf(slider.help()));
    }
    let hovered = presentation.pause_menu_interaction.hovered?;
    if hovered == PauseMenuButton::Keyboard {
        return Some(LauncherStatusHelp::Csf(IN_GAME_KEYBOARD_HELP));
    }
    template.control(hovered)?.help.map(LauncherStatusHelp::Csf)
}

fn hit(state: &AppState, layout: &PauseMenuLayout) -> Option<PauseMenuButton> {
    let (x, y) = crate::app::frontend::skirmish_shell_render::in_game_shell_cursor(state);
    let states = button_states(state);
    layout
        .buttons()
        .find(|(button, rect)| states[*button as usize].enabled && rect.contains(x, y))
        .map(|(button, _)| button)
}

pub(crate) fn cursor_moved(state: &mut AppState) {
    let Some(layout) = current_layout(state) else {
        return;
    };
    if options_page_pointer(state, None) {
        return;
    }
    let (x, y) = crate::app::frontend::skirmish_shell_render::in_game_shell_cursor(state);
    let over = hit(state, &layout);
    let pause = &mut state.match_state.match_presentation.internet_pause;
    // A held trackbar owns the capture (`0x0061E4DF`): nothing else sees the
    // pointer until release.
    if let Some(hold) = pause.hold {
        let changed = match (hold.dragging, layout.internet) {
            (true, Some(controls)) => {
                let rect = controls.sliders[hold.id as usize];
                let value = hold.id.range().value_at_in(x, rect, ShellScale::RETAIL);
                pause.set(hold.id, value)
            }
            _ => false,
        };
        if changed {
            App::play_skirmish_shell_generic_click_sound(state);
        }
        return;
    }
    pause.hovered = layout.internet.and_then(|c| c.slider_at(x, y));
    state
        .match_state
        .match_presentation
        .pause_menu_interaction
        .hovered = over;
}

pub(crate) fn mouse(state: &mut AppState, button: MouseButton, pressed: bool) {
    if button != MouseButton::Left {
        return;
    }
    let Some(layout) = current_layout(state) else {
        return;
    };
    let (x, y) = crate::app::frontend::skirmish_shell_render::in_game_shell_cursor(state);
    if options_page_pointer(state, Some(pressed)) {
        return;
    }
    if !pressed
        && state
            .match_state
            .match_presentation
            .internet_pause
            .hold
            .take()
            .is_some()
    {
        return;
    }
    if pressed && let Some(controls) = layout.internet {
        if let Some(slider) = controls.slider_at(x, y) {
            let rect = controls.sliders[slider as usize];
            let pause = &mut state.match_state.match_presentation.internet_pause;
            let press = slider.range().press_in(
                pause.positions[slider as usize],
                x,
                y,
                rect,
                ShellScale::RETAIL,
            );
            pause.hold = press.hold(slider);
            if let TrackbarPress::Jump(value) = press
                && pause.set(slider, value)
            {
                App::play_skirmish_shell_generic_click_sound(state);
            }
            return;
        }
    }
    let over = hit(state, &layout);
    let interaction = &mut state.match_state.match_presentation.pause_menu_interaction;
    interaction.hovered = over;
    if pressed {
        interaction.press(over);
        if over.is_some() {
            // Owner-draw button-down: GUIMainButtonSound (Rules+0x188,
            // 0x0061374B..0x00613771).
            App::play_main_menu_button_sound(state);
        }
    } else if let Some(button) = interaction.release(over) {
        activate(state, button);
    }
}

fn activate(state: &mut AppState, button: PauseMenuButton) {
    match button {
        // gamemd's 4F11B0 writes state 5 here and BBB opens as its own
        // dialog. VERA shows BBB's settings as the menu's first page.
        PauseMenuButton::GameControls => {
            if let Some(page) = state.match_state.match_presentation.options_page.as_mut() {
                page.dialog.select_category(IN_GAME_LANDING);
            }
        }
        PauseMenuButton::Resume => {
            resume_internet_events(state);
            App::enter_in_game_menu_state(state, InGameMenuState::Closed)
        }
        // 0x522 (`0x004F16C0..0x004F16EA`): GameMode 4 writes state 2 while
        // `[0xB779C4]` is set, every other game state 3.
        PauseMenuButton::Abort => {
            if state
                .match_state
                .sim_runtime
                .runtime()
                .is_some_and(|runtime| runtime.simulation.session().wol_tournament_game())
            {
                crate::app::input::surrender::open(state);
            } else {
                App::enter_in_game_menu_state(state, InGameMenuState::AbortConfirm)
            }
        }
        PauseMenuButton::Load => App::open_saved_game_browser(state, SavedSeedMode::Load),
        PauseMenuButton::Save => App::open_saved_game_browser(state, SavedSeedMode::Save),
        PauseMenuButton::Delete => App::open_saved_game_browser(state, SavedSeedMode::Delete),
        PauseMenuButton::SharedPause => {
            toggle_multiplayer_pause(state);
        }
        // BBB's 0x52C: A3 opens over the menu and comes back to it.
        PauseMenuButton::Keyboard => crate::app::input::keyboard::open(
            state,
            crate::ui::shell::keyboard::KeyboardParent::GameControls,
        ),
    }
}

/// What FF's Resume (`0x004F1534..0x004F16BB`) emits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct InternetResume {
    pub(crate) latency_fudge: Option<i32>,
    pub(crate) game_speed: Option<i32>,
}

/// In mode4 with Session+0x300 clear, each changed trackbar emits its event
/// only while its limiter has expired, then restarts that limiter for 900
/// buckets; a refused change only logs. Speed is read after latency.
///
/// VERA deviation: a relay-paced match emits neither event. The relay keeps
/// the lobby's frame rate and never reads LatencyFudge, so a speed event
/// would only rescale SpeedNormalize's animation delays against an unchanged
/// pace. The sliders still move and reopen at the live values.
pub(crate) fn internet_resume(
    controls: &InternetPauseControls,
    latency_fudge: i32,
    game_speed: i32,
    limiters: &mut PauseEventLimiters,
    now_bucket: u32,
    relay_paced: bool,
) -> InternetResume {
    let mut out = InternetResume::default();
    if relay_paced {
        return out;
    }
    let fudge = 3 - controls.positions[InternetSlider::Connection as usize];
    if fudge != latency_fudge {
        if limiters.latency.remaining(now_bucket) == 0 {
            limiters.latency =
                WallBucketTimer::start(now_bucket, PauseEventLimiters::COOLDOWN_BUCKETS);
            out.latency_fudge = Some(fudge);
        } else {
            log::info!("Ignoring latency change to avoid congestion");
        }
    }
    let speed = crate::ui::shell::game_speed::SpeedContext::Network
        .stored_speed(controls.positions[InternetSlider::Speed as usize]);
    if speed != game_speed {
        if limiters.speed.remaining(now_bucket) == 0 {
            limiters.speed =
                WallBucketTimer::start(now_bucket, PauseEventLimiters::COOLDOWN_BUCKETS);
            out.game_speed = Some(speed);
        } else {
            log::info!("Ignoring speed change to avoid congestion");
        }
    }
    out
}

/// Resume re-reads GameMode and Session+0x300 (4F1527..4F153B): a defeat
/// while FF is open drops both events. Only FF holds the trackbars.
fn resume_internet_events(state: &mut AppState) {
    let local_defeated = state
        .match_state
        .sim_runtime
        .runtime()
        .is_some_and(|runtime| runtime.simulation.local_player_defeated());
    if current_template(state) != PauseMenuTemplate::Internet
        || state.match_state.session_mode() != SessionMode::Wol
        || local_defeated
    {
        return;
    }
    let controls = state.match_state.match_presentation.internet_pause;
    let latency_fudge = state.match_state.session.latency_fudge();
    let game_speed = current_game_speed(state);
    let now_bucket = crate::app::match_runtime::frame_pacer::wall_clock_bucket();
    let relay_paced = state
        .match_state
        .live_network
        .as_ref()
        .is_some_and(|live| live.relay_frames_per_second().is_some());
    let resume = internet_resume(
        &controls,
        latency_fudge,
        game_speed,
        state.match_state.session.pause_event_limiters_mut(),
        now_bucket,
        relay_paced,
    );
    let Some(owner) = crate::app::input::commands::preferred_local_owner(state) else {
        return;
    };
    // Resume4F1595..4F167A emits latency first, then speed. The limiters above
    // have already advanced even when OutList refuses either event.
    if let Some(value) = resume.latency_fudge {
        crate::app::input::commands::schedule_command(
            state,
            &owner,
            crate::sim::command::Command::SetLatencyFudge { value },
        );
    }
    if let Some(speed) = resume.game_speed {
        crate::app::input::commands::schedule_command(
            state,
            &owner,
            crate::sim::command::Command::LiveGameSpeed { speed: speed as u8 },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A menu opened and closed with nothing changed leaves VERA.INI alone: a
    /// capture's stand-in options, for one, must never reach the file.
    #[test]
    fn only_a_moved_control_or_live_change_commits_the_page() {
        use crate::ui::main_menu_dialogs::options::{LauncherOptionsLabels, LauncherTrackbarId};
        let profile = RetailOptionsProfile::default();
        let dialog = crate::app::persistence::options::launcher::launcher_dialog_from_profile(
            &profile,
            LauncherOptionsLabels::resolve(&|_| None),
            false,
            false,
            true,
            Vec::new(),
        );
        let opened = dialog.pack();
        let mut page = InGameOptionsPage {
            dialog,
            opened,
            opened_profile: profile.clone(),
            game_speed_shown: true,
            opened_speed_position: 0,
        };
        assert!(page.untouched(&profile));
        assert_eq!(page.moved_speed_position(), None);

        // A moved Game Speed applies on close, unless BBB's init hid the row.
        page.dialog
            .set_in_game_controls(String::new(), Vec::new(), 6, String::new());
        assert_eq!(page.moved_speed_position(), Some(6));
        page.game_speed_shown = false;
        assert_eq!(page.moved_speed_position(), None);
        page.game_speed_shown = true;
        page.dialog
            .set_in_game_controls(String::new(), Vec::new(), 0, String::new());

        page.dialog.select_category(Category::Display);
        assert!(page.untouched(&profile), "a tab only shows other controls");

        let live = RetailOptionsProfile {
            shift_queue: !profile.shift_queue,
            ..profile.clone()
        };
        assert!(!page.untouched(&live), "a list or box that applies at once");

        let tab = |row| crate::ui::shell::geom::RectPx::new(653, 227 + row * 25, 125, 25);
        let layout = LauncherOptionsLayout::in_game(
            (800, 600),
            &[tab(1), tab(2)],
            Category::Gameplay,
            false,
            true,
            false,
            (800, 600),
        );
        let detail = layout
            .trackbar_rect(LauncherTrackbarId::Detail)
            .expect("Visual Details is on this page");
        let (x, y) = (detail.x + 5, detail.y + detail.h - 3);
        page.dialog.shell_mouse_down(x, y, &layout);
        page.dialog.shell_mouse_up(x, y, &layout);
        assert!(!page.untouched(&profile), "a control the close commits");
    }

    #[test]
    fn shared_pause_control_waits_for_confirmation_and_remains_resumable() {
        let mut pause = crate::net::live_commands::MultiplayerPauseStatus {
            available: true,
            paused: false,
            frame: 42,
            epoch: 0,
            pending: false,
        };
        let running = multiplayer_control_for(pause, false).unwrap();
        assert!(running.enabled);
        assert_eq!(running.caption, "VERA:PauseAllPlayers");
        pause.pending = true;
        assert!(!multiplayer_control_for(pause, false).unwrap().enabled);
        pause.paused = true;
        pause.pending = false;
        assert!(!multiplayer_control_for(pause, false).unwrap().enabled);
        let stopped = multiplayer_control_for(pause, true).unwrap();
        assert!(stopped.enabled);
        assert_eq!(stopped.caption, "VERA:ResumeAllPlayers");
        pause.pending = true;
        assert_eq!(
            multiplayer_control_for(pause, true).unwrap().caption,
            "VERA:Resuming"
        );
        pause.available = false;
        assert!(multiplayer_control_for(pause, true).is_none());
    }

    #[test]
    fn chat_pause_commands_ask_only_for_a_change_the_control_allows() {
        let mut pause = crate::net::live_commands::MultiplayerPauseStatus {
            available: true,
            ..Default::default()
        };
        let running = multiplayer_control_for(pause, false);
        assert_eq!(
            chat_command_for(running, false, true),
            (true, Some("VERA:PauseRequested"))
        );
        // `/resume` with nothing paused does nothing.
        assert_eq!(chat_command_for(running, false, false), (false, None));
        pause.paused = true;
        let stopped = multiplayer_control_for(pause, true);
        assert_eq!(
            chat_command_for(stopped, true, false),
            (true, Some("VERA:ResumeRequested"))
        );
        assert_eq!(
            chat_command_for(stopped, true, true),
            (false, Some("VERA:PausedForAllPlayers"))
        );
        // The barrier frame is not finished here yet: no resume.
        assert_eq!(
            chat_command_for(multiplayer_control_for(pause, false), true, false),
            (false, Some("VERA:FinishingPause"))
        );
        assert_eq!(
            chat_command_for(None, false, true),
            (false, Some("VERA:PauseNotAvailable"))
        );
    }

    #[test]
    fn ff_init_projects_latency_and_speed_onto_the_trackbars() {
        let controls = InternetPauseControls::open(1, 3);
        assert_eq!(controls.positions, [2, 3]);
        // Init's range/position notifications set both labels at open.
        assert_eq!(
            controls.value_caption(InternetSlider::Connection).0,
            "TXT_GOOD_CONNECTION"
        );
        assert_eq!(
            controls.value_caption(InternetSlider::Speed).0,
            "TXT_MEDIUM"
        );
        // TBM_SETPOS ignores an out-of-range value.
        assert_eq!(InternetPauseControls::open(5, 9).positions, [0, 0]);
    }

    #[test]
    fn a_changed_position_updates_the_right_static() {
        let mut controls = InternetPauseControls::open(1, 3);
        assert!(!controls.set(InternetSlider::Speed, 3));
        assert!(controls.set(InternetSlider::Speed, 7));
        assert_eq!(
            controls.value_caption(InternetSlider::Speed).0,
            "VERA:GameSpeed90"
        );
        assert!(controls.set(InternetSlider::Connection, 0));
        assert_eq!(
            controls.value_caption(InternetSlider::Connection).0,
            "TXT_WORST_CONNECTION"
        );
    }

    #[test]
    fn resume_rate_limits_each_event_for_900_buckets() {
        let mut limiters = PauseEventLimiters::default();
        let mut controls = InternetPauseControls::open(1, 3);
        // Unchanged values emit nothing and arm nothing.
        assert_eq!(
            internet_resume(&controls, 1, 3, &mut limiters, 100, false),
            InternetResume::default()
        );
        controls.set(InternetSlider::Connection, 3);
        controls.set(InternetSlider::Speed, 5);
        assert_eq!(
            internet_resume(&controls, 1, 3, &mut limiters, 100, false),
            InternetResume {
                latency_fudge: Some(0),
                game_speed: Some(1),
            }
        );
        // Within 900 buckets both changes are refused.
        assert_eq!(
            internet_resume(&controls, 2, 4, &mut limiters, 999, false),
            InternetResume::default()
        );
        assert_eq!(
            internet_resume(&controls, 2, 4, &mut limiters, 1000, false),
            InternetResume {
                latency_fudge: Some(0),
                game_speed: Some(1),
            }
        );
    }

    #[test]
    fn a_relay_paced_match_sends_neither_slider_event() {
        let mut limiters = PauseEventLimiters::default();
        let mut controls = InternetPauseControls::open(1, 3);
        controls.set(InternetSlider::Connection, 3);
        controls.set(InternetSlider::Speed, 5);
        assert_eq!(
            internet_resume(&controls, 1, 3, &mut limiters, 100, true),
            InternetResume::default()
        );
        // Nothing was sent, so no limiter was armed.
        assert_eq!(
            internet_resume(&controls, 1, 3, &mut limiters, 101, false),
            InternetResume {
                latency_fudge: Some(0),
                game_speed: Some(1),
            }
        );
    }
}
