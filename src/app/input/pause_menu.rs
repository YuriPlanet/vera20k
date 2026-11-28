//! In-game menu input. Native callback4F11B0 owns the commands of B5, BBA and
//! FF; rendering and hit-testing share the loaded side-art geometry
//! (72FC60,60B000,60B350,60B7A0).

use crate::app::App;
use crate::app::PlatformState;
use crate::app::audio_runtime::AppAudioRuntime;
use crate::app::diagnostics::performance_overlay::PerformanceOverlay;
use crate::app::diagnostics::state::DiagnosticsState;
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::session::{PauseEventLimiters, SessionMode, WallBucketTimer};
use crate::app::match_runtime::sim_host::SimHost;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::persistence::options_profile::RetailOptionsProfile;
use crate::app::presentation::state::MatchPresentationState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::audio::sfx::SfxPlayer;
use crate::ui::main_menu_dialogs::OptionsDialogState;
use crate::ui::main_menu_dialogs::options::shell::{IN_GAME_KEYBOARD_HELP, LauncherOptionsLayout};
use crate::ui::main_menu_dialogs::options::{
    Category, LauncherCheckboxId, LauncherOptionsEvent, LauncherOptionsPacked,
    LauncherParentResult, LauncherTrackbarId,
};
use crate::ui::pause_menu::InGameMenuState;
use crate::ui::shell::page::PageEvent;
use crate::ui::shell::pause_menu::{
    self, InternetSlider, PauseMenuButton, PauseMenuButtonState, PauseMenuLayout,
    PauseMenuTemplate, pause_menu_layout,
};
use crate::ui::skirmish_shell::SavedSeedMode;
use winit::window::Window;

/// FF's two trackbars' positions as the dialog holds them between init and
/// Resume; the trackbars report each move here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct InternetPauseControls {
    /// Trackbar values in [`InternetSlider`] order.
    pub(crate) positions: [i32; 2],
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
/// that button (4F11B0 writes state 5). Its Options button shows the main
/// menu's pages with their own column.
#[derive(Debug)]
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
fn refit_match(
    platform: &PlatformState,
    renderer: &mut RendererState,
    diag: &DiagnosticsState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &PersistenceState,
) {
    App::sync_zoom_pass(platform, renderer, &persistence.options_profile);
    crate::app::presentation::sidebar_render::refresh_sidebar_projection(
        platform,
        renderer,
        diag,
        frontend,
        match_state,
        process_assets,
        audio,
        persistence,
    );
}

/// The template the open in-game menu was created from.
pub(crate) fn current_template(presentation: &MatchPresentationState) -> PauseMenuTemplate {
    presentation.pause_menu_template
}

/// `0x004F10E1..0x004F1130` for the running session.
fn select_template(match_state: &MatchState) -> PauseMenuTemplate {
    let mode = match_state.session_mode();
    let local_defeated = match_state
        .sim_runtime
        .runtime()
        .is_some_and(|runtime| runtime.simulation.local_player_defeated());
    PauseMenuTemplate::select(
        mode.is_single_player(),
        mode == SessionMode::Wol,
        local_defeated,
    )
}

pub(crate) fn current_layout(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &MatchState,
    persistence: &PersistenceState,
) -> Option<PauseMenuLayout> {
    let shell = crate::app::frontend::skirmish_shell_render::current_in_game_shell_layout(
        platform,
        renderer,
        frontend,
        match_state,
        persistence,
    )?;
    let mut layout = pause_menu_layout(
        shell.width,
        shell.height,
        shell.layout,
        shell.button_size,
        current_template(&match_state.match_presentation),
    );
    if multiplayer_control(match_state).is_some() {
        layout.enable_shared_pause();
    }
    let category = match_state
        .match_presentation
        .options_page
        .as_ref()
        .map(|page| page.dialog.category());
    // Game Controls has Options under it; the other pages have the main
    // menu's column in place of the menu's buttons.
    match category {
        Some(Category::GameControls) | None => layout.add_options_button(),
        Some(_) => layout.show_options_column(),
    }
    // FF's group heads the Game Options page and has no place on the others.
    match category {
        Some(Category::Gameplay) => layout.raise_internet_group(),
        Some(_) => layout.internet = None,
        None => {}
    }
    Some(layout)
}

/// The Options page showing on the open in-game menu, in the shell's pixels.
pub(crate) fn options_page_layout(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &MatchState,
    persistence: &PersistenceState,
) -> Option<LauncherOptionsLayout> {
    let page = match_state.match_presentation.options_page.as_ref()?;
    let shell = crate::app::frontend::skirmish_shell_render::current_in_game_shell_layout(
        platform,
        renderer,
        frontend,
        match_state,
        persistence,
    )?;
    let battle = App::match_zoom_geometry(platform, &renderer.gpu, &persistence.options_profile);
    let menu = current_layout(platform, renderer, frontend, match_state, persistence)?;
    Some(
        LauncherOptionsLayout::in_game(
            (shell.width, shell.height),
            menu.page_column(),
            page.dialog.category(),
            current_template(&match_state.match_presentation) == PauseMenuTemplate::Internet,
            page.game_speed_shown,
            menu.button(PauseMenuButton::Keyboard).is_none(),
            (renderer.gpu.config.width, renderer.gpu.config.height),
        )
        .with_battle_frame((battle.src_width, battle.src_height)),
    )
}

/// What the page's controls did takes effect through the main menu's own
/// path. A match has to be fitted to a new frame as a window resize does.
fn run_options_page(
    platform: &mut PlatformState,
    renderer: &mut RendererState,
    diag: &DiagnosticsState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &mut ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &mut PersistenceState,
) {
    let Some(page) = match_state.match_presentation.options_page.as_mut() else {
        return;
    };
    let output = page.dialog.drain_output();
    let edit_layout = output
        .events
        .contains(&LauncherOptionsEvent::EditLayoutRequested);
    // The Options column's Keyboard is the main menu's own button, whose
    // parent result opens the in-game key page here.
    let child = output
        .events
        .iter()
        .find_map(|event| match event {
            LauncherOptionsEvent::InGameKeyboardRequested => Some(InGameMenuState::Keyboard),
            LauncherOptionsEvent::InGameSoundRequested => Some(InGameMenuState::Sound),
            _ => None,
        })
        .or((output.result == Some(LauncherParentResult::Keyboard))
            .then_some(InGameMenuState::Keyboard));
    let refit = output.events.iter().any(|event| {
        matches!(
            event,
            LauncherOptionsEvent::ZoomSelected(_)
                | LauncherOptionsEvent::SidebarSelected(_)
                | LauncherOptionsEvent::SidebarColumnsSelected(_)
                | LauncherOptionsEvent::Toggled(
                    LauncherCheckboxId::OriginalShape
                        | LauncherCheckboxId::HdRadar
                        | LauncherCheckboxId::SmoothSidebar,
                    _
                )
        )
    });
    let language = App::run_launcher_options_events(
        platform,
        renderer,
        frontend,
        match_state,
        process_assets,
        audio,
        persistence,
        output.cues,
        output.events,
    );
    if refit {
        refit_match(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
        );
    }
    if language {
        let labels = App::launcher_options_labels(frontend.shell_route, process_assets);
        if let Some(page) = match_state.match_presentation.options_page.as_mut() {
            page.dialog.set_labels(labels);
        }
    }
    // BBB's 0x52C and 0x52D: the dialogs open over the menu, which closes
    // and commits its page on the way, and come back to it.
    match child {
        Some(InGameMenuState::Keyboard) => crate::app::input::keyboard::open(
            platform,
            renderer,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
            crate::ui::shell::keyboard::KeyboardParent::GameControls,
        ),
        Some(InGameMenuState::Sound) => crate::app::input::sound::open(
            platform,
            renderer,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
        ),
        _ => {}
    }
    // The match ignores the mouse while the menu is open, so Edit Layout
    // closes it as Resume does before the mode takes the mouse.
    if edit_layout {
        activate(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
            PauseMenuButton::Resume,
        );
        crate::app::input::layout_edit::start(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            &mut audio.sfx_player,
            persistence,
        );
    }
}

/// The pointer over the Options page: `pressed` is the left button's new
/// state, `None` a move. True when the menu's other controls don't get to see
/// it: an open list or a held slider has the pointer, or the page has gone.
fn options_page_pointer(
    platform: &mut PlatformState,
    renderer: &mut RendererState,
    diag: &DiagnosticsState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &mut ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &mut PersistenceState,
    pressed: Option<bool>,
) -> bool {
    let Some(layout) = options_page_layout(platform, renderer, frontend, match_state, persistence)
    else {
        return false;
    };
    let (x, y) = crate::app::frontend::skirmish_shell_render::in_game_shell_cursor(
        platform,
        renderer,
        &frontend.screen,
        match_state,
        persistence,
    );
    let Some(page) = match_state.match_presentation.options_page.as_mut() else {
        return false;
    };
    let was_taken = page.dialog.shell_pointer_taken();
    match pressed {
        Some(true) => page.dialog.shell_mouse_down(x, y, &layout),
        Some(false) => page.dialog.shell_mouse_up(x, y, &layout),
        None => page.dialog.shell_mouse_move(x, y, &layout),
    }
    run_options_page(
        platform,
        renderer,
        diag,
        frontend,
        match_state,
        process_assets,
        audio,
        persistence,
    );
    was_taken || match_state.match_presentation.options_page.is_none()
}

/// Esc on a page of the Options column goes back to Game Controls and the
/// menu's own buttons, as the column's Back does. False on Game Controls.
pub(crate) fn back_from_options(presentation: &mut MatchPresentationState) -> bool {
    let Some(page) = presentation.options_page.as_mut() else {
        return false;
    };
    if page.dialog.category() == Category::GameControls {
        return false;
    }
    page.dialog.select_category(Category::GameControls);
    true
}

/// Esc closes the page's open list before anything else hears it.
pub(crate) fn close_options_list(
    platform: &mut PlatformState,
    renderer: &mut RendererState,
    diag: &DiagnosticsState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &mut ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &mut PersistenceState,
) -> bool {
    let closed = match_state
        .match_presentation
        .options_page
        .as_mut()
        .is_some_and(|page| page.dialog.shell_close_list());
    if closed {
        run_options_page(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
        );
    }
    closed
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct MultiplayerPauseControl {
    /// [`crate::i18n`] labels, resolved where they are drawn.
    pub(crate) caption: &'static str,
    pub(crate) enabled: bool,
    /// The pause's notice, a message row while it lasts
    /// (`messages::show_status_notices`).
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

pub(crate) fn multiplayer_control(match_state: &MatchState) -> Option<MultiplayerPauseControl> {
    let pause = match_state.live_network.as_ref()?.multiplayer_pause()?;
    multiplayer_control_for(pause, match_state.multiplayer_paused())
}

/// The explicit relay action is serviced without simulation ticks. Repeats
/// while awaiting confirmation are consumed, never queued as toggles.
pub(crate) fn toggle_multiplayer_pause(window: &Window, match_state: &mut MatchState) -> bool {
    let Some(control) = multiplayer_control(match_state) else {
        return false;
    };
    if control.enabled
        && let Some(pause) = live_pause(match_state)
    {
        request_multiplayer_pause(window, match_state, !pause.paused);
    }
    true
}

fn live_pause(
    match_state: &MatchState,
) -> Option<crate::net::live_commands::MultiplayerPauseStatus> {
    match_state.live_network.as_ref()?.multiplayer_pause()
}

/// Asks the relay to pause or resume, unless the match is ending. True when
/// the request went out.
fn request_multiplayer_pause(window: &Window, match_state: &mut MatchState, paused: bool) -> bool {
    let sent = match_state.scenario_exit.is_none()
        && match_state.scenario_outcome.is_none()
        && match_state
            .live_network
            .as_mut()
            .is_some_and(|live| live.request_multiplayer_pause(paused));
    if sent {
        window.request_redraw();
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
pub(crate) fn chat_command(
    platform: &PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    sfx_player: &mut Option<SfxPlayer>,
    persistence: &PersistenceState,
    want: bool,
) {
    let paused = live_pause(match_state).is_some_and(|pause| pause.paused);
    let (ask, answer) = chat_command_for(multiplayer_control(match_state), paused, want);
    if let Some(label) = answer
        && (!ask || request_multiplayer_pause(&platform.window, match_state, want))
    {
        crate::app::input::messages::post_chat_command_answer(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            sfx_player,
            persistence,
            &crate::i18n::tr(label),
        );
    }
}

/// The applied GameSpeed FF's init (4F1720) reads; a queued speed event
/// changes it only when it executes.
fn current_game_speed(sim_runtime: &SimHost) -> i32 {
    sim_runtime.runtime().map_or(0, |runtime| {
        runtime.simulation.session().game_options().game_speed
    })
}

/// Entering state 1 recreates the dialog: 4F10E0 picks its template and FF's
/// init reads the live values.
pub(crate) fn on_open(
    platform: &PlatformState,
    performance_overlay: &PerformanceOverlay,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    launcher_audio_available: bool,
    options_profile: &RetailOptionsProfile,
) {
    match_state.match_presentation.pause_menu_template = select_template(match_state);
    match_state.match_presentation.internet_pause = InternetPauseControls::open(
        match_state.session.latency_fudge(),
        current_game_speed(&match_state.sim_runtime),
    );
    // Escape opens on Game Controls, the settings a player most often reaches
    // for mid-match. Back from one of the menu's dialogs shows the page it
    // left (`App::enter_in_game_menu_state` forgets it on the way to play).
    let category = match_state
        .match_presentation
        .options_page_category
        .unwrap_or(Category::GameControls);
    // BBB's init (4E1FE0, message 0x497) decides the Game Speed row and
    // reads the speed in effect.
    crate::app::input::in_game_options::on_open(frontend, match_state);
    let options = &match_state.match_presentation.in_game_options;
    let (context, game_speed_shown) = (options.speed_context, !options.game_speed_hidden);
    let speed_position = u8::try_from(options.game_speed_position()).unwrap_or(0);
    let label = |key: &str, fallback: &str| process_assets.csf_label(key, fallback);
    let speed_names = (0..=crate::ui::shell::game_speed::MAX_POSITION)
        .map(|position| label(context.label_key(context.stored_speed(position)), ""))
        .collect();
    let speed_caption = label("GUI:GameSpeed", "Game Speed");
    let sound_label = label("GUI:Sound", "Sound");
    let mut dialog = App::launcher_options_from_state(
        platform,
        performance_overlay,
        frontend.shell_route,
        process_assets,
        launcher_audio_available,
        options_profile,
    );
    dialog.set_in_game_controls(speed_caption, speed_names, speed_position, sound_label);
    dialog.select_category(category);
    let opened = dialog.pack();
    let opened_profile = options_profile.clone();
    match_state.match_presentation.options_page = Some(InGameOptionsPage {
        opened_speed_position: dialog.trackbar_position(LauncherTrackbarId::GameSpeed),
        dialog,
        opened,
        opened_profile,
        game_speed_shown,
    });
}

/// The menu is closing: what its page holds applies and is saved before the
/// next page reads those values.
pub(crate) fn on_close(
    platform: &mut PlatformState,
    renderer: &RendererState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &mut PersistenceState,
) {
    close_page(
        platform,
        renderer,
        frontend,
        match_state,
        process_assets,
        audio,
        persistence,
        true,
    );
}

/// The match is ending, or the program quitting, with the menu open. The
/// page's settings are still saved, but a moved Game Speed is dropped: only
/// BBB's Back result (1) applies and writes it through 4E1DE0, its result 2
/// does neither, and a speed command would reach a session that is over.
pub(crate) fn on_match_end(
    platform: &mut PlatformState,
    renderer: &RendererState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &mut PersistenceState,
) {
    close_page(
        platform,
        renderer,
        frontend,
        match_state,
        process_assets,
        audio,
        persistence,
        false,
    );
}

fn close_page(
    platform: &mut PlatformState,
    renderer: &RendererState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &mut PersistenceState,
    apply_speed: bool,
) {
    let Some(page) = match_state.match_presentation.options_page.take() else {
        return;
    };
    match_state.match_presentation.options_page_category = Some(page.dialog.category());
    // A moved Game Speed is BBB's accepted speed (4E1DE0): the profile and
    // the match take it, and the commit below writes the profile once.
    let speed = page.moved_speed_position().filter(|_| apply_speed);
    if let Some(position) = speed {
        let options = &mut match_state.match_presentation.in_game_options;
        let stored = options.speed_context.stored_speed(i32::from(position));
        options.game_speed = u32::try_from(stored).unwrap_or(0);
        persistence.options_profile.game_speed = stored;
        crate::app::persistence::options::apply_in_game_speed(match_state);
    }
    if speed.is_some() || !page.untouched(&persistence.options_profile) {
        App::commit_in_game_options_page(
            platform,
            renderer,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
            page.dialog,
            page.opened,
        );
    }
}

pub(crate) fn button_states(match_state: &MatchState) -> [PauseMenuButtonState; 8] {
    let ui = &match_state.match_presentation.dialog_ui;
    let template = current_template(&match_state.match_presentation);
    // Native4F17D9..4F1832 applies559C20 eligibility to Load/Delete only, and
    // only in modes0/5; BBA/FF have neither control.
    let has_saves = match_state.match_presentation.pause_menu_has_saves;
    let controls_page = match_state
        .match_presentation
        .options_page
        .as_ref()
        .is_some_and(|page| page.dialog.category() == Category::GameControls);
    PauseMenuButton::ALL.map(|button| PauseMenuButtonState {
        pressed: super::dialog_page::pressed(ui, template.id(button)),
        // C5 timer highlighting is separate from pointer hover. VERA lights
        // Game Controls while its page shows, as a tab.
        highlighted: button == PauseMenuButton::GameControls && controls_page,
        enabled: if button == PauseMenuButton::SharedPause {
            multiplayer_control(match_state).is_some_and(|control| control.enabled)
        } else {
            has_saves || !matches!(button, PauseMenuButton::Load | PauseMenuButton::Delete)
        },
    })
}

/// The status-line help for the control under the pointer: the Options
/// page's, else the menu's ([`super::dialog_page::help`]; a held trackbar
/// keeps the help it showed at the press, FF's pointer moves going only to
/// it until the release). A string-table or `VERA:` key.
pub(crate) fn hovered_help(presentation: &MatchPresentationState) -> Option<&str> {
    if let Some(help) = presentation
        .options_page
        .as_ref()
        .and_then(|page| page.dialog.shell_status_help())
    {
        return Some(help);
    }
    super::dialog_page::help(&presentation.dialog_ui, None)
}

/// Puts the menu's live values on its nodes: each button's enable and lit
/// look (`states`), FF's trackbar positions.
fn sync_values(
    gui: &mut vera20k_gui::Gui,
    root: vera20k_gui::NodeId,
    template: PauseMenuTemplate,
    states: &[PauseMenuButtonState; 8],
    positions: [i32; 2],
) {
    use crate::ui::shell::page::set;
    use vera20k_gui::{Prop, Value};
    // A button the template lacks has id 0 and no node.
    for button in PauseMenuButton::ALL
        .into_iter()
        .filter(|b| template.id(*b) != 0)
    {
        let (id, look) = (template.id(button), states[button as usize]);
        set(gui, root, id, Prop::Enabled, Value::Bool(look.enabled));
        set(gui, root, id, Prop::Toggled, Value::Bool(look.highlighted));
    }
    for slider in InternetSlider::ALL {
        let position = Value::Int(positions[slider as usize]);
        set(gui, root, slider.id(), Prop::Pos, position);
    }
}

/// Makes the menu's nodes current without input, every button on its rect
/// (a disabled one too, which paints in the side's dark colour) in its
/// `states`: its renderer's call every drawn frame before it paints from
/// them. The pointer's own sync hides the disabled buttons, so they neither
/// hover nor take a press. The open Options page's tree is brought in line
/// with its layout too, which it returns.
pub(crate) fn sync(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    persistence: &PersistenceState,
    states: &[PauseMenuButtonState; 8],
) -> Option<(PauseMenuLayout, Option<LauncherOptionsLayout>)> {
    let layout = current_layout(platform, renderer, frontend, match_state, persistence)?;
    let page_layout = options_page_layout(platform, renderer, frontend, match_state, persistence);
    if let (Some(page_layout), Some(page)) = (
        &page_layout,
        match_state.match_presentation.options_page.as_mut(),
    ) {
        page.dialog.shell_sync(page_layout);
    }
    let template = layout.template;
    let positions = match_state.match_presentation.internet_pause.positions;
    super::dialog_page::sync_dialog(
        &mut match_state.match_presentation.dialog_ui,
        template.dialog(),
        || pause_menu::spec(template, positions, IN_GAME_KEYBOARD_HELP),
        |id| layout.rect(id),
        |gui, root| sync_values(gui, root, template, states, positions),
    );
    Some((layout, page_layout))
}

/// One pointer message on the menu (`down`: `None` moves). The Options page
/// sees it first; a disabled button neither hovers nor takes a press.
pub(super) fn pointer(
    platform: &mut PlatformState,
    renderer: &mut RendererState,
    diag: &DiagnosticsState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &mut ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &mut PersistenceState,
    down: Option<bool>,
) {
    let Some(layout) = current_layout(platform, renderer, frontend, match_state, persistence)
    else {
        return;
    };
    if options_page_pointer(
        platform,
        renderer,
        diag,
        frontend,
        match_state,
        process_assets,
        audio,
        persistence,
        down,
    ) {
        return;
    }
    let states = button_states(match_state);
    let template = layout.template;
    let positions = match_state.match_presentation.internet_pause.positions;
    let event = super::dialog_page::shell_event(
        platform,
        renderer,
        &frontend.screen,
        match_state,
        persistence,
        down,
    );
    let ui = &mut match_state.match_presentation.dialog_ui;
    let rect = |id| {
        let enabled = template
            .button(id)
            .is_none_or(|button| states[button as usize].enabled);
        layout.rect(id).filter(|_| enabled)
    };
    let events = super::dialog_page::route(
        ui,
        template.dialog(),
        || pause_menu::spec(template, positions, IN_GAME_KEYBOARD_HELP),
        rect,
        |gui, root| sync_values(gui, root, template, &states, positions),
        event,
    );
    // Owner-draw button-down: GUIMainButtonSound (Rules+0x188,
    // 0x0061374B..0x00613771).
    for event in super::dialog_page::play_cues(
        frontend,
        match_state,
        process_assets,
        &mut audio.sfx_player,
        events,
    ) {
        match event {
            PageEvent::Activated(id) => {
                if let Some(button) = template.button(id) {
                    activate(
                        platform,
                        renderer,
                        diag,
                        frontend,
                        match_state,
                        process_assets,
                        audio,
                        persistence,
                        button,
                    );
                }
            }
            // A moved trackbar's notification sets its right static.
            PageEvent::Slid {
                id,
                value,
                final_: false,
            } => {
                if let Some(slider) = InternetSlider::from_id(id)
                    && match_state
                        .match_presentation
                        .internet_pause
                        .set(slider, value)
                {
                    App::play_page_cue(
                        frontend,
                        match_state,
                        process_assets,
                        &mut audio.sfx_player,
                        crate::ui::shell::page::PageCue::ListClick,
                    );
                }
            }
            _ => {}
        }
    }
}

fn activate(
    platform: &mut PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &mut PersistenceState,
    button: PauseMenuButton,
) {
    match button {
        // gamemd's 4F11B0 writes state 5 here and BBB opens as its own
        // dialog. VERA shows BBB's settings as the menu's first page.
        PauseMenuButton::GameControls => {
            if let Some(page) = match_state.match_presentation.options_page.as_mut() {
                page.dialog.select_category(Category::GameControls);
            }
        }
        PauseMenuButton::Resume => {
            resume_internet_events(
                platform,
                renderer,
                diag,
                frontend,
                match_state,
                process_assets,
                &mut audio.sfx_player,
                persistence,
            );
            App::enter_in_game_menu_state(
                platform,
                renderer,
                frontend,
                match_state,
                process_assets,
                audio,
                persistence,
                InGameMenuState::Closed,
            )
        }
        // 0x522 (`0x004F16C0..0x004F16EA`): GameMode 4 writes state 2 while
        // `[0xB779C4]` is set, every other game state 3.
        PauseMenuButton::Abort => {
            if match_state
                .sim_runtime
                .runtime()
                .is_some_and(|runtime| runtime.simulation.session().wol_tournament_game())
            {
                crate::app::input::surrender::open(
                    platform,
                    renderer,
                    diag,
                    frontend,
                    match_state,
                    process_assets,
                    audio,
                    persistence,
                );
            } else {
                App::enter_in_game_menu_state(
                    platform,
                    renderer,
                    frontend,
                    match_state,
                    process_assets,
                    audio,
                    persistence,
                    InGameMenuState::AbortConfirm,
                )
            }
        }
        PauseMenuButton::Load => App::open_saved_game_browser(
            platform,
            renderer,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
            SavedSeedMode::Load,
        ),
        PauseMenuButton::Save => App::open_saved_game_browser(
            platform,
            renderer,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
            SavedSeedMode::Save,
        ),
        PauseMenuButton::Delete => App::open_saved_game_browser(
            platform,
            renderer,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
            SavedSeedMode::Delete,
        ),
        PauseMenuButton::SharedPause => {
            toggle_multiplayer_pause(&platform.window, match_state);
        }
        // BBB's 0x52C: A3 opens over the menu and comes back to it.
        PauseMenuButton::Keyboard => crate::app::input::keyboard::open(
            platform,
            renderer,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
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
fn resume_internet_events(
    platform: &PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    sfx_player: &mut Option<SfxPlayer>,
    persistence: &PersistenceState,
) {
    let local_defeated = match_state
        .sim_runtime
        .runtime()
        .is_some_and(|runtime| runtime.simulation.local_player_defeated());
    if current_template(&match_state.match_presentation) != PauseMenuTemplate::Internet
        || match_state.session_mode() != SessionMode::Wol
        || local_defeated
    {
        return;
    }
    let controls = match_state.match_presentation.internet_pause;
    let latency_fudge = match_state.session.latency_fudge();
    let game_speed = current_game_speed(&match_state.sim_runtime);
    let now_bucket = crate::app::match_runtime::frame_pacer::wall_clock_bucket();
    let relay_paced = match_state
        .live_network
        .as_ref()
        .is_some_and(|live| live.relay_frames_per_second().is_some());
    let resume = internet_resume(
        &controls,
        latency_fudge,
        game_speed,
        match_state.session.pause_event_limiters_mut(),
        now_bucket,
        relay_paced,
    );
    let Some(owner) = crate::app::input::commands::preferred_local_owner(match_state) else {
        return;
    };
    // Resume4F1595..4F167A emits latency first, then speed. The limiters above
    // have already advanced even when OutList refuses either event.
    if let Some(value) = resume.latency_fudge {
        crate::app::input::commands::schedule_command(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            sfx_player,
            persistence,
            &owner,
            crate::sim::command::Command::SetLatencyFudge { value },
        );
    }
    if let Some(speed) = resume.game_speed {
        crate::app::input::commands::schedule_command(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            sfx_player,
            persistence,
            &owner,
            crate::sim::command::Command::LiveGameSpeed { speed: speed as u8 },
        );
    }
}

#[cfg(all(test, tests_app_input_d_z))]
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
        assert!(page.untouched(&profile), "a page only shows other controls");

        let live = RetailOptionsProfile {
            shift_queue: !profile.shift_queue,
            ..profile.clone()
        };
        assert!(!page.untouched(&live), "a list or box that applies at once");

        let row = |row| crate::ui::shell::geom::RectPx::new(653, 227 + row * 25, 125, 25);
        let column: Vec<_> = (0..8).map(row).collect();
        let layout = LauncherOptionsLayout::in_game(
            (800, 600),
            &column,
            Category::Display,
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
