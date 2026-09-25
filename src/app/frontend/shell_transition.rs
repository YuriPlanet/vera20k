//! Generic shell first-paint slide driver (menu / single-player / skirmish).
//!
//! The original slides every allow-listed shell dialog in on its first paint and
//! out when it closes — not a menu->skirmish edge transition and not a
//! whole-screen crossfade. The slide engine animates the dialog's right-panel
//! column (every tile row's SDBTNANM frame, plus the Skirmish map button and top
//! panel) on a 30 ms tick; nothing is repositioned.
//!
//! The render-agnostic data + schedule live in [`crate::ui::shell::slide`] (each
//! rendered dialog's slide column and the [`ShellFrameWave`] frame sweep). This module is the app/render glue: it maps the currently-showing
//! screen to a shell dialog, (re)starts/advances the wave on entry edges, plays
//! the slide-in start cue, and dispatches the per-frame shell repaint while the
//! wave is live. The slide-in start cue is `GUIMoveInSound` (stock `MenuSlideIn`);
//! the stock-empty end cue (`ShellButtonSlideSound`) stays silent.

use std::time::{Duration, Instant};

use anyhow::{Result, bail};

use crate::app::AppState;
use crate::ui::shell::descriptor::DialogId;

// Re-export the render-agnostic schedule types from the shared substrate so the
// shell renderers (and the `AppState` field) keep their existing import paths.
pub(crate) use crate::ui::shell::slide::{
    ColumnDraw, MainMenuEntryPaintFrame, MainMenuEntryPresentToken, PanelArt, PresentedPoll,
    ShellFrameWave, SlideColumn,
};

pub(crate) enum ShellFirstPaintRenderResult {
    NotRendered,
    Rendered {
        main_menu_entry_token: Option<MainMenuEntryPresentToken>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MainMenuFirstPaintPoll {
    Acquire,
    WaitUntil(Instant),
    Completed,
}

/// Which shell dialog a first-paint slide belongs to. Every allow-listed shell
/// dialog slides on its own first paint; this identifies the one currently
/// showing so the trigger can detect entry edges and look up its slide column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShellSlideKind {
    /// Dialog 0xE2 — main menu.
    MainMenu,
    /// Dialog 0x100 — single-player page.
    SinglePlayer,
    /// Dialog 0x101 — Movies & Credits page.
    MoviesAndCredits,
    /// Dialog 0x129 — movie list (Play Movie and Back).
    MovieList,
    /// Dialog 0x102 — offline skirmish setup.
    Skirmish,
    /// Dialog 0x94 — campaign selection.
    Campaign,
    /// Dialog 0xB7 — Single Player's Load Saved Game.
    LoadSavedGame,
    /// Dialog 0xD5 — launcher Options.
    Options,
    /// Dialog 0x10E — Westwood Online welcome.
    WolWelcome,
    /// Dialog 0xA3 — Options' Keyboard page (front-end parent only).
    Keyboard,
    /// Dialog 0x6B — Skirmish's Choose Map, a family page of its own.
    ChooseMap,
    /// Dialog 0x108 — the score screen after a skirmish game.
    Score,
}

impl ShellSlideKind {
    /// The Win32 dialog resource id this shell maps to. The slide's eligibility
    /// and column are looked up from this id in the data-driven `slide` table
    /// (no hardcoded per-kind counts here).
    pub(crate) fn dialog_id(self) -> DialogId {
        DialogId(match self {
            ShellSlideKind::MainMenu => 0x00E2,
            ShellSlideKind::SinglePlayer => 0x0100,
            ShellSlideKind::MoviesAndCredits => 0x0101,
            ShellSlideKind::MovieList => 0x0129,
            ShellSlideKind::Skirmish => 0x0102,
            ShellSlideKind::Campaign => 0x0094,
            ShellSlideKind::LoadSavedGame => 0x00B7,
            ShellSlideKind::Options => 0x00D5,
            ShellSlideKind::WolWelcome => 0x010E,
            ShellSlideKind::Keyboard => 0x00A3,
            ShellSlideKind::ChooseMap => 0x006B,
            ShellSlideKind::Score => 0x0108,
        })
    }

    /// The dialog's slide column with the panel's `rows` tile rows. Every
    /// rendered shell has a `slide` table entry, so a miss is a programming
    /// error.
    pub(crate) fn column(self, rows: u32) -> SlideColumn {
        crate::ui::shell::slide::slide_spec_for(self.dialog_id())
            .expect("rendered shell dialog must have a slide column")
            .column(rows)
    }
}

/// Right-panel tile rows at the current surface size (`[0x00B0FA20]`).
fn panel_rows(state: &AppState) -> u32 {
    let config = &state.renderer.gpu.config;
    crate::ui::shell::geom::right_panel_rects(config.width as i32, config.height as i32)
        .tile_count
        .max(1) as u32
}

/// What a family dialog's teardown leads to once its slide-out has run: the
/// dialog proc's result, dispatched to the state that owns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ShellExitThen {
    MainMenu(crate::ui::main_menu_shell::MainMenuShellAction),
    SinglePlayer(crate::ui::single_player_shell::SinglePlayerShellAction),
    MoviesCredits(crate::ui::movies_credits_shell::MoviesCreditsAction),
    /// Movie list Play Movie with a selected row.
    PlayMovie,
    /// Movie list Back (state `0xE` result -1).
    MovieListBack,
    /// Skirmish Start Game (result `0x617`) with the session packed and
    /// resolved when it was pressed.
    SkirmishStart(Box<crate::skirmish_launch::SkirmishLaunchSession>),
    /// Skirmish Back (result `0x5C0`) to the Single Player page.
    SkirmishBack,
    /// Campaign selection Back (result -1): state 1 recreates Single Player.
    CampaignBack,
    /// Load Saved Game Back (result 2): state 1 recreates Single Player.
    LoadSavedGameBack,
    /// An Options result: `0x0055FC80` tears `0xD5` down with its slide, then
    /// commits and writes (Main Menu `0x5CB`; state 0x12 recreates `0xE2`) or
    /// commits and runs the Keyboard page (`0x5CE`).
    Options(crate::ui::main_menu_dialogs::options::LauncherParentResult),
    /// Keyboard `0xA3` Back or Cancel: `0x005FBEF0` tears it down with its
    /// slide, then the bindings save or reload and a new `0xD5` is built.
    KeyboardClose(crate::app::input::keyboard::KeyboardExit),
    /// Skirmish Choose Map (`0x5AA`): `0x102` slides out and hides without
    /// packing (`0x006AD931`, `0x006AD93C`), then `0x6B` runs.
    SkirmishChooseMap,
    /// Choose Map Use Map: `0x6B` slides out (`0x007757E0`), the selection
    /// commits and `0x102` shows again with its entry slide.
    ChooseMapUse(crate::ui::skirmish_shell::ChooseMapSelection),
    /// Choose Map Cancel: `0x6B` slides out and `0x102` shows again.
    ChooseMapCancel,
    /// Choose Map Create Random Map: `0x6B` slides out and hides
    /// (`0x005E6A03..0x005E6A0B`) before the random-map dialog runs.
    ChooseMapRandomMap,
    /// Westwood Online Main Menu (result 0): `0xE2` is recreated.
    WolBack,
    /// Score Continue (result 1, `0x005CA06C`): `0x108` slides out, the
    /// match ends and PrepareSession resumes the shell.
    ScoreContinue,
    /// A Westwood Online action: `0x10E` closes before the WOLAPI object
    /// fails to load and `TXT_APIMISSING` shows.
    WolApiMissing,
}

impl ShellExitThen {
    /// The dialog whose result this is.
    pub(crate) fn dialog(&self) -> ShellSlideKind {
        match self {
            Self::MainMenu(_) => ShellSlideKind::MainMenu,
            Self::SinglePlayer(_) => ShellSlideKind::SinglePlayer,
            Self::MoviesCredits(_) => ShellSlideKind::MoviesAndCredits,
            Self::PlayMovie | Self::MovieListBack => ShellSlideKind::MovieList,
            Self::SkirmishStart(_) | Self::SkirmishBack => ShellSlideKind::Skirmish,
            Self::CampaignBack => ShellSlideKind::Campaign,
            Self::LoadSavedGameBack => ShellSlideKind::LoadSavedGame,
            Self::Options(_) => ShellSlideKind::Options,
            Self::KeyboardClose(_) => ShellSlideKind::Keyboard,
            Self::SkirmishChooseMap => ShellSlideKind::Skirmish,
            Self::ChooseMapUse(_) | Self::ChooseMapCancel | Self::ChooseMapRandomMap => {
                ShellSlideKind::ChooseMap
            }
            Self::WolBack | Self::WolApiMissing => ShellSlideKind::WolWelcome,
            Self::ScoreContinue => ShellSlideKind::Score,
        }
    }
}

/// A shown family dialog's teardown slide (`0x00622720 -> 0x00608070`): the
/// buttons ramp out on the entry schedule while input is blocked, then the
/// dialog is destroyed and `then` runs.
#[derive(Debug, Clone)]
pub(crate) struct ShellExit {
    wave: ShellFrameWave,
    then: ShellExitThen,
    /// When the last tick had been shown.
    completed_at: Option<Instant>,
}

/// After tearing `0x94` down, state 8 waits while the campaign voice still
/// plays, at most this long (`0x0052E036..0x0052E089`, `0xBB8` ms).
const CAMPAIGN_VOICE_WAIT: Duration = Duration::from_millis(3000);

/// How a request to leave a family dialog starts.
#[derive(Debug)]
pub(crate) enum ShellExitStart {
    /// The teardown slide runs; the result follows when it ends.
    Sliding,
    /// The dialog is not showing steady, so `0x00608070` returns without a
    /// slide: the returned result runs at once.
    Immediate(ShellExitThen),
    /// The dialog is already sliding out with a result; this one is dropped.
    AlreadyLeaving,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExitStartRule {
    Slide,
    Immediate,
    AlreadyLeaving,
}

/// How leaving dialog `kind` starts, given the running exit, the showing
/// target, the dialog whose slide state is live and whether its entry slide
/// still runs.
fn exit_start_rule(
    exit_running: bool,
    target: Option<ShellSlideKind>,
    active: Option<ShellSlideKind>,
    entry_running: bool,
    kind: ShellSlideKind,
) -> ExitStartRule {
    if exit_running {
        ExitStartRule::AlreadyLeaving
    } else if target == Some(kind) && active == Some(kind) && !entry_running {
        ExitStartRule::Slide
    } else {
        ExitStartRule::Immediate
    }
}

/// Begin the teardown slide of the dialog that produced `then`.
pub(crate) fn begin_shell_exit(state: &mut AppState, then: ShellExitThen) -> ShellExitStart {
    let kind = then.dialog();
    match exit_start_rule(
        state.frontend.shell_exit.is_some(),
        current_shell_slide_target(state),
        state.frontend.shell_slide_active_shell,
        state.frontend.shell_first_paint_slide.is_some(),
        kind,
    ) {
        ExitStartRule::Slide => {
            let column = kind.column(panel_rows(state));
            state.frontend.shell_exit = Some(ShellExit {
                wave: ShellFrameWave::new_slide_out(column, Instant::now()),
                then,
                completed_at: None,
            });
            ShellExitStart::Sliding
        }
        ExitStartRule::Immediate => ShellExitStart::Immediate(then),
        ExitStartRule::AlreadyLeaving => ShellExitStart::AlreadyLeaving,
    }
}

impl ShellExit {
    /// Advance one 30 ms tick when due; true once every tick has been shown
    /// (`0x006071E0` draws ticks `0..bound`, then the teardown continues).
    fn advance(&mut self, now: Instant) -> bool {
        self.wave.advance(now);
        self.wave.is_complete()
    }
}

/// Advance the running teardown slide; returns its result once the last tick
/// has been shown. A dialog that stopped showing while it slid out (another
/// route replaced it, such as a game loaded from an overlay panel) takes its
/// result with it.
pub(crate) fn advance_shell_exit(state: &mut AppState, now: Instant) -> Option<ShellExitThen> {
    let kind = state.frontend.shell_exit.as_ref()?.then.dialog();
    if current_shell_slide_target(state) != Some(kind) {
        state.frontend.shell_exit = None;
        return None;
    }
    if !state.frontend.shell_exit.as_mut()?.advance(now) {
        return None;
    }
    if kind == ShellSlideKind::Campaign {
        // The last slide-out frame stays on screen while the voice plays.
        let completed_at = *state
            .frontend
            .shell_exit
            .as_mut()?
            .completed_at
            .get_or_insert(now);
        if now < completed_at + CAMPAIGN_VOICE_WAIT
            && crate::app::App::campaign_voice_playing(state)
        {
            return None;
        }
    }
    state.frontend.shell_exit.take().map(|exit| exit.then)
}

/// The teardown slide of `kind` while it runs.
pub(crate) fn shell_exit_wave(state: &AppState, kind: ShellSlideKind) -> Option<&ShellFrameWave> {
    state
        .frontend
        .shell_exit
        .as_ref()
        .filter(|exit| exit.then.dialog() == kind)
        .map(|exit| &exit.wave)
}

/// Shell capture only: keep the running teardown slide at its current tick.
pub(crate) fn hold_shell_exit_for_capture(state: &mut AppState) {
    if let Some(exit) = state.frontend.shell_exit.as_mut() {
        exit.wave.hold_for_capture();
    }
}

/// Either slide of the showing family dialog runs: the slide engine draws
/// only the button frames (no captions), the RA2TS static shows no movie and
/// no static timer is delivered (`0x006071E0` sleeps without dispatching).
/// Both slides belong to the showing dialog: an entry slide is armed for the
/// current target and an exit is dropped once its dialog stops showing.
pub(crate) fn shell_slide_running(state: &AppState) -> bool {
    state.frontend.shell_first_paint_slide.is_some() || state.frontend.shell_exit.is_some()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShellEntryEffect {
    Unchanged,
    LeftShells,
    Started(ShellSlideKind),
}

#[derive(Debug)]
enum ShellWaveCompletion {
    /// Single Player `0x100`, Movies & Credits `0x101` or movie list `0x129`.
    MenuPage,
    Skirmish,
    /// Score `0x108`: the heading, the status line and the table start.
    Score,
}

/// Render-agnostic reducer for dialog-instance, entry-wave, and title state.
///
/// Production route mutations and the frame driver both use this reducer, so a
/// dialog destroyed between paints cannot be hidden from an edge detector that
/// only remembers the last rendered target.
struct ShellLifecycleReducer<'a> {
    /// Right-panel tile rows for a new wave's column.
    panel_rows: u32,
    active_shell: &'a mut Option<ShellSlideKind>,
    first_paint_slide: &'a mut Option<ShellFrameWave>,
    slide_generation: &'a mut u64,
    title_reveal: &'a mut crate::ui::shell::static_reveal::Kind1StaticReveal,
    monitor: &'a mut crate::ui::shell::warning_monitor::WarningMonitor,
    page_title: &'a mut crate::ui::shell::static_reveal::PresentedKind1Static,
    status_line: &'a mut crate::ui::shell::static_reveal::PresentedKind1Static,
}

impl<'a> ShellLifecycleReducer<'a> {
    fn from_state(state: &'a mut AppState) -> Self {
        Self {
            panel_rows: panel_rows(state),
            active_shell: &mut state.frontend.shell_slide_active_shell,
            first_paint_slide: &mut state.frontend.shell_first_paint_slide,
            slide_generation: &mut state.frontend.shell_slide_generation,
            title_reveal: &mut state.frontend.main_menu_shell_state.title_reveal,
            monitor: &mut state.frontend.shell_monitor,
            page_title: &mut state.frontend.shell_page_title,
            status_line: &mut state.frontend.shell_status_line,
        }
    }

    fn invalidate_main_menu_dialog_instance(&mut self) {
        self.title_reveal.reset_waiting();
        *self.active_shell = None;
        *self.first_paint_slide = None;
    }

    fn observe_target(&mut self, target: Option<ShellSlideKind>, now: Instant) -> ShellEntryEffect {
        if target == *self.active_shell {
            return ShellEntryEffect::Unchanged;
        }
        if target == Some(ShellSlideKind::MainMenu)
            || *self.active_shell == Some(ShellSlideKind::MainMenu)
        {
            self.title_reveal.reset_waiting();
        }
        *self.active_shell = target;
        match target {
            Some(kind) => {
                // A new dialog instance creates new statics: 0x71C at frame 0
                // with its timer unarmed (0x0060A982), hidden 0x694 and 0x695.
                *self.monitor = Default::default();
                self.page_title.reset();
                self.status_line.reset();
                *self.first_paint_slide = Some(if kind == ShellSlideKind::MainMenu {
                    *self.slide_generation = self.slide_generation.wrapping_add(1);
                    if *self.slide_generation == 0 {
                        *self.slide_generation = 1;
                    }
                    ShellFrameWave::new_presented_main_menu(
                        *self.slide_generation,
                        kind.column(self.panel_rows),
                    )
                } else {
                    ShellFrameWave::new_first_paint_slide(kind.column(self.panel_rows), now)
                });
                ShellEntryEffect::Started(kind)
            }
            None => {
                *self.first_paint_slide = None;
                ShellEntryEffect::LeftShells
            }
        }
    }

    fn advance_wave(&mut self, now: Instant) {
        if let Some(wave) = self.first_paint_slide.as_mut() {
            wave.advance(now);
        }
    }

    fn finish_completed_wave(&mut self, kind: ShellSlideKind) -> Option<ShellWaveCompletion> {
        if !self
            .first_paint_slide
            .as_ref()
            .is_some_and(ShellFrameWave::is_complete)
        {
            return None;
        }
        *self.first_paint_slide = None;
        Some(match kind {
            ShellSlideKind::MainMenu => return None,
            ShellSlideKind::SinglePlayer
            | ShellSlideKind::MoviesAndCredits
            | ShellSlideKind::MovieList
            | ShellSlideKind::Campaign
            | ShellSlideKind::LoadSavedGame
            | ShellSlideKind::Options
            | ShellSlideKind::WolWelcome
            | ShellSlideKind::Keyboard
            | ShellSlideKind::ChooseMap => ShellWaveCompletion::MenuPage,
            ShellSlideKind::Skirmish => ShellWaveCompletion::Skirmish,
            ShellSlideKind::Score => ShellWaveCompletion::Score,
        })
    }

    fn complete_presented_main_menu(&mut self, generation: u64, title: &str, now: Instant) -> bool {
        if *self.active_shell != Some(ShellSlideKind::MainMenu)
            || !self
                .first_paint_slide
                .as_ref()
                .is_some_and(|wave| wave.is_presented_completing(generation))
        {
            if let Some(wave) = self.first_paint_slide.as_mut() {
                wave.poison_presented();
            }
            return false;
        }
        if !self.title_reveal.start(title, now)
            || !self
                .first_paint_slide
                .as_ref()
                .is_some_and(|wave| wave.is_presented_completing(generation))
        {
            if let Some(wave) = self.first_paint_slide.as_mut() {
                wave.poison_presented();
            }
            return false;
        }
        // The same SHOW completion (0x0060AA60) starts status line 0x695.
        self.status_line.start(now);
        *self.first_paint_slide = None;
        true
    }
}

/// Invalidate the destroyed/recreated 0xE2 dialog instance at an actual route
/// boundary, even when the destination never reaches a paint.
///
/// `shell_slide_active_shell` remembers the last target observed by the frame
/// driver. Clearing it here makes a collapsed 0xE2 -> 0x100 -> Back round trip
/// produce a fresh 0xE2 entry edge instead of inheriting the old terminal title.
pub(crate) fn invalidate_main_menu_dialog_instance(state: &mut AppState) {
    ShellLifecycleReducer::from_state(state).invalidate_main_menu_dialog_instance();
    // A new 0xE2 starts with no press or hover: its status line stays empty
    // until a hover message arrives from the real cursor position.
    let menu = &mut state.frontend.main_menu_shell_state;
    menu.pressed_owner_draw_button = None;
    menu.hovered_owner_draw_button = None;
    if state.frontend.shell_controller.top_id() == Some(DialogId(0x00E2)) {
        state
            .frontend
            .shell_controller
            .reset_to(DialogId(0x00E2), false);
    }
}

/// Deliver the kind-1 timer only while the bare 0xE2 dialog owns steady paint.
/// Waiting state is inert, so the terminal slide frame cannot begin the title
/// before its own successful presentation.
pub(crate) fn poll_main_menu_title_reveal(state: &mut AppState) {
    if current_shell_slide_target(state) == Some(ShellSlideKind::MainMenu)
        && state.frontend.shell_first_paint_slide.is_none()
    {
        state
            .frontend.main_menu_shell_state
            .title_reveal
            .poll_timer(Instant::now());
    }
}

pub(crate) fn current_main_menu_entry_frame(state: &AppState) -> Option<MainMenuEntryPaintFrame> {
    state
        .frontend.shell_first_paint_slide
        .as_ref()
        .and_then(ShellFrameWave::current_main_menu_frame)
}

/// Commit one exact main-menu generation/tick after `output.present()`.
/// Any impossible mismatch poisons the already-visible wave before returning.
pub(crate) fn record_main_menu_entry_presented(
    state: &mut AppState,
    token: MainMenuEntryPresentToken,
) -> Result<()> {
    let generation = token.generation();
    let tick = token.tick();
    if current_shell_slide_target(state) != Some(ShellSlideKind::MainMenu) {
        if let Some(wave) = state.frontend.shell_first_paint_slide.as_mut() {
            wave.poison_presented();
        }
        bail!(
            "main-menu present token {generation}:{tick} committed after route ownership changed"
        );
    }
    let Some(wave) = state.frontend.shell_first_paint_slide.as_mut() else {
        bail!("main-menu present token {generation}:{tick} has no active wave");
    };
    if let Err(error) = wave.record_presented(token, Instant::now()) {
        wave.poison_presented();
        bail!("main-menu present token {generation}:{tick} rejected: {error}");
    }
    Ok(())
}

pub(crate) fn blocks_shell_input(state: &AppState) -> bool {
    // The graceful quit cascade also freezes shell input (the original processes
    // no input during its blocking teardown), so a stray click can't re-enter the
    // menu mid-fade.
    state.frontend.quit_cascade.is_some()
        || state.frontend.shell_exit.is_some()
        || transition_blocks_shell_input(state.frontend.shell_first_paint_slide.as_ref())
}

pub(crate) fn transition_blocks_shell_input(transition: Option<&ShellFrameWave>) -> bool {
    transition.is_some()
}

pub(crate) fn main_menu_presented_wake_deadline(state: &AppState) -> Option<Instant> {
    state
        .frontend.shell_first_paint_slide
        .as_ref()
        .and_then(ShellFrameWave::presented_wake_deadline)
}

pub(crate) fn main_menu_presented_is_poisoned(state: &AppState) -> bool {
    state
        .frontend.shell_first_paint_slide
        .as_ref()
        .is_some_and(ShellFrameWave::is_presented_poisoned)
}

/// Which allow-listed shell dialog is currently showing, if any. Mirrors the
/// main-menu render dispatch order (skirmish > single-player > bare menu); the
/// egui fallback / skirmish-setup paths are not native shell dialogs and do not
/// slide. The candidate is gated on its dialog having a slide column. Returns
/// the score dialog on the result screen and `None` off the shell screens.
pub(crate) fn current_shell_slide_target(state: &AppState) -> Option<ShellSlideKind> {
    use crate::ui::game_screen::GameScreen;
    // The score dialog runs after the game, before the shell resumes.
    if matches!(state.frontend.screen, GameScreen::MissionResult { .. }) {
        return (crate::app::App::score_shell_active(state)
            && crate::ui::shell::slide::is_slide_eligible(ShellSlideKind::Score.dialog_id()))
        .then_some(ShellSlideKind::Score);
    }
    if state.frontend.screen != GameScreen::MainMenu {
        return None;
    }
    // Play_Movie and Show_Credits run after their source dialog is destroyed
    // and before the next one exists: no shell dialog is showing.
    if state.frontend.fullscreen_movie.is_some() || state.frontend.credits_roll.is_some() {
        return None;
    }
    // Options `0xD5` runs after `0xE2` is destroyed (state 5,
    // `0x0052DDAB`); state 0x12 builds a new `0xE2` when it closes. Its
    // Keyboard page `0xA3` runs after `0xD5` is destroyed (`0x0055FD06`) and
    // slides like it. The Exit confirmation (state 6) and the quit after it
    // (state 7) run without a family dialog.
    if let Some(dialog) = state.frontend.keyboard_dialog.as_ref() {
        return (dialog.parent == crate::ui::shell::keyboard::KeyboardParent::Launcher)
            .then_some(ShellSlideKind::Keyboard)
            .filter(|kind| crate::ui::shell::slide::is_slide_eligible(kind.dialog_id()));
    }
    if state.frontend.exit_confirm_modal.is_some() || state.frontend.quit_cascade.is_some() {
        return None;
    }
    // Only the native page slides; the assetless fallback does not.
    if state.frontend.options_dialog.is_some() {
        return (crate::app::App::native_launcher_options_active(state)
            && crate::ui::shell::slide::is_slide_eligible(ShellSlideKind::Options.dialog_id()))
        .then_some(ShellSlideKind::Options);
    }
    let candidate =
        if state.frontend.shell_route.skirmish() || state.frontend.dev_skirmish_shell_enabled {
            let shell = &state.frontend.skirmish_shell_state;
            if shell.choose_map_modal.is_none() {
                ShellSlideKind::Skirmish
            } else if shell.random_map_setup_modal.is_some() || shell.saved_seed_browser.is_some() {
                // The chooser hides while the random-map dialogs run.
                return None;
            } else {
                ShellSlideKind::ChooseMap
            }
        } else if state.frontend.shell_route.single_player() {
            ShellSlideKind::SinglePlayer
        } else if state.frontend.shell_route.movies_and_credits() {
            ShellSlideKind::MoviesAndCredits
        } else if state.frontend.shell_route.movie_list() {
            ShellSlideKind::MovieList
        } else if state.frontend.shell_route.campaign() {
            ShellSlideKind::Campaign
        } else if state.frontend.shell_route.load_saved_game() {
            ShellSlideKind::LoadSavedGame
        } else if state.frontend.shell_route.wol_welcome() {
            // The TXT_APIMISSING box runs after 0x10E is destroyed.
            if state
                .frontend
                .wol_welcome
                .as_ref()
                .is_some_and(|wol| wol.api_missing.is_some())
            {
                return None;
            }
            ShellSlideKind::WolWelcome
        } else if !state.frontend.main_menu_shell_failed {
            ShellSlideKind::MainMenu
        } else {
            return None;
        };
    crate::ui::shell::slide::is_slide_eligible(candidate.dialog_id()).then_some(candidate)
}

/// Arm a newly created `0xE2` before swapchain acquisition. This deliberately
/// does not play the entry cue or expose tick 0.
pub(crate) fn prepare_main_menu_first_paint_before_acquire(state: &mut AppState) {
    let target = current_shell_slide_target(state);
    if target == Some(ShellSlideKind::MainMenu) {
        if state.frontend.shell_slide_active_shell != target {
            ShellLifecycleReducer::from_state(state).observe_target(target, Instant::now());
        }
    } else if state.frontend.shell_slide_active_shell == Some(ShellSlideKind::MainMenu) {
        ShellLifecycleReducer::from_state(state).observe_target(None, Instant::now());
    }
}

/// Poll the exact `0xE2` clock before acquiring another surface.
pub(crate) fn poll_main_menu_first_paint_before_acquire(
    state: &mut AppState,
    now: Instant,
) -> Result<MainMenuFirstPaintPoll> {
    if current_shell_slide_target(state) != Some(ShellSlideKind::MainMenu) {
        return Ok(MainMenuFirstPaintPoll::Acquire);
    }
    let Some(wave) = state.frontend.shell_first_paint_slide.as_mut() else {
        return Ok(MainMenuFirstPaintPoll::Acquire);
    };
    let Some(poll) = wave.poll_presented(now) else {
        return Ok(MainMenuFirstPaintPoll::Acquire);
    };
    match poll {
        PresentedPoll::Acquire => Ok(MainMenuFirstPaintPoll::Acquire),
        PresentedPoll::WaitUntil(deadline) => Ok(MainMenuFirstPaintPoll::WaitUntil(deadline)),
        PresentedPoll::Poisoned => bail!("main-menu first-paint wave is poisoned"),
        PresentedPoll::Complete => {
            let generation = wave
                .presented_generation()
                .expect("presented poll has generation");
            // Active-retail stock leaves ShellButtonSlideSound empty, but the
            // completion hook remains a named lifecycle edge.
            crate::app::App::play_shell_slide_completion_sound(state);
            let title = crate::app::frontend::main_menu_shell_render::main_menu_title_text(state).into_owned();
            if !ShellLifecycleReducer::from_state(state)
                .complete_presented_main_menu(generation, &title, now)
            {
                bail!("main-menu generation {generation} failed its completion transaction");
            }
            Ok(MainMenuFirstPaintPoll::Completed)
        }
    }
}

/// Activate an armed `0xE2` only after successful swapchain acquisition.
/// Compatibility dialogs retain their prior post-acquisition entry behavior.
pub(crate) fn activate_shell_first_paint_after_acquire(state: &mut AppState) {
    let target = current_shell_slide_target(state);
    if target == Some(ShellSlideKind::MainMenu) {
        if state
            .frontend.shell_first_paint_slide
            .as_mut()
            .is_some_and(ShellFrameWave::activate_after_acquire)
        {
            crate::app::App::play_shell_slide_in_sound(state);
        }
        return;
    }
    let effect = ShellLifecycleReducer::from_state(state).observe_target(target, Instant::now());
    if let ShellEntryEffect::Started(_) = effect {
        crate::app::App::play_shell_slide_in_sound(state);
    }
}

/// Render the currently-showing shell while its first-paint slide is live, then
/// advance/complete the wave. Returns `Rendered` when it owned the frame. The shell
/// renderer reads `state.shell_first_paint_slide` and paints the slide column in
/// place of the buttons, without the children the slide leaves blank.
pub(crate) fn render_shell_first_paint_slide(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> Result<ShellFirstPaintRenderResult> {
    if state.frontend.shell_first_paint_slide.is_none() {
        return Ok(ShellFirstPaintRenderResult::NotRendered);
    }
    let Some(kind) = current_shell_slide_target(state) else {
        // No eligible shell is showing; drop the stale wave and let the normal
        // dispatch paint this frame.
        state.frontend.shell_first_paint_slide = None;
        return Ok(ShellFirstPaintRenderResult::NotRendered);
    };

    if kind == ShellSlideKind::MainMenu {
        let frame = current_main_menu_entry_frame(state)
            .ok_or_else(|| anyhow::anyhow!("main-menu entry acquired without a ready frame"))?;
        return match crate::app::frontend::main_menu_shell_render::render_main_menu_first_paint_frame(
            state,
            encoder,
            destination,
            frame,
        )? {
            crate::app::frontend::main_menu_shell_render::MainMenuEntryRenderResult::Rendered { token } => {
                Ok(ShellFirstPaintRenderResult::Rendered {
                    main_menu_entry_token: Some(token),
                })
            }
            crate::app::frontend::main_menu_shell_render::MainMenuEntryRenderResult::Fallback => {
                state.frontend.shell_first_paint_slide = None;
                Ok(ShellFirstPaintRenderResult::NotRendered)
            }
        };
    }

    ShellLifecycleReducer::from_state(state).advance_wave(Instant::now());

    let rendered = match kind {
        ShellSlideKind::Skirmish | ShellSlideKind::ChooseMap => {
            if !crate::app::App::ensure_skirmish_shell_chrome(state) {
                log::warn!("Skirmish shell chrome unavailable; cancelling first-paint slide");
                state.frontend.shell_first_paint_slide = None;
                return Ok(ShellFirstPaintRenderResult::NotRendered);
            }
            let color = state.renderer.shell_surface_presenter.source_render_view();
            let depth = state.renderer.depth_view.clone();
            crate::app::frontend::skirmish_shell_render::render_skirmish_shell_to_target(
                state,
                encoder,
                crate::render::shell_transition_pass::ShellRenderTarget {
                    color: &color,
                    depth: &depth,
                },
                crate::app::frontend::skirmish_shell_render::ShellRenderMode::TransitionPreview,
            )?;
            state
                .renderer.shell_surface_presenter
                .encode_present(encoder, destination);
            true
        }
        ShellSlideKind::MovieList => {
            crate::app::frontend::movies_credits_render::render_movie_list(
                state,
                encoder,
                destination,
            )?
        }
        ShellSlideKind::Campaign => {
            crate::app::frontend::campaign_shell_render::render_campaign_page(
                state,
                encoder,
                destination,
            )?
        }
        ShellSlideKind::LoadSavedGame => {
            crate::app::frontend::load_saved_game_render::render_load_saved_game_page(
                state,
                encoder,
                destination,
            )?
        }
        ShellSlideKind::WolWelcome => {
            crate::app::frontend::wol_welcome_render::render_wol_welcome_page(
                state,
                encoder,
                destination,
            )?
        }
        ShellSlideKind::Options => {
            crate::app::App::native_launcher_options_active(state) && {
                crate::app::frontend::skirmish_shell_render::render_launcher_options(
                    state,
                    encoder,
                    destination,
                )?;
                true
            }
        }
        ShellSlideKind::Keyboard => {
            crate::app::frontend::skirmish_shell_render::render_keyboard_shell(
                state,
                encoder,
                destination,
            )?;
            true
        }
        ShellSlideKind::Score => crate::app::frontend::score_shell_render::render_score_page(
            state,
            encoder,
            destination,
        )?,
        ShellSlideKind::SinglePlayer | ShellSlideKind::MoviesAndCredits => matches!(
            crate::app::frontend::menu_page_render::render_active_menu_page(
                state,
                encoder,
                destination,
            )?,
            crate::app::frontend::menu_page_render::MenuPageRenderResult::Rendered
        ),
        ShellSlideKind::MainMenu => unreachable!("handled above"),
    };

    if !rendered {
        // Shell fell back (assets missing): abandon the slide so the normal
        // dispatch can render the fallback path with its egui overlays.
        state.frontend.shell_first_paint_slide = None;
        return Ok(ShellFirstPaintRenderResult::NotRendered);
    }

    let completion = ShellLifecycleReducer::from_state(state).finish_completed_wave(kind);
    match completion {
        // `0x102`'s SHOW completion: its own statics start, or repaint when
        // the dialog shows again after Choose Map.
        Some(ShellWaveCompletion::Skirmish) => {
            let now = Instant::now();
            let (title, game_type, map_label) =
                crate::app::frontend::skirmish_shell_render::skirmish_right_panel_label_strings(state);
            state
                .frontend
                .skirmish_shell_state
                .statics
                .show(&title, &game_type, &map_label, now);
        }
        // Menu pages start their heading reveal on the same edge (0xE2 starts
        // its own in the presented-entry completion transaction).
        Some(ShellWaveCompletion::MenuPage) => {
            let now = Instant::now();
            let title = crate::app::frontend::menu_page_render::active_page_title_text(state, kind);
            let frontend = &mut state.frontend;
            frontend.shell_page_title.set_text(&title, now);
            frontend.shell_page_title.start(now);
            frontend.shell_status_line.start(now);
        }
        // 0x108's SHOW completion starts every kind-1 static at once: the
        // heading, the status line and the 47 table texts.
        Some(ShellWaveCompletion::Score) => {
            let now = Instant::now();
            let title = crate::app::frontend::score_shell_render::score_title_text(state);
            let frontend = &mut state.frontend;
            frontend.shell_page_title.set_text(&title, now);
            frontend.shell_page_title.start(now);
            frontend.shell_status_line.start(now);
            if let Some(page) = frontend.score_page.as_mut() {
                page.start_reveals(now);
            }
        }
        None => {}
    }

    Ok(ShellFirstPaintRenderResult::Rendered {
        main_menu_entry_token: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::shell::static_reveal::{Kind1PaintWindow, Kind1StaticReveal};
    use std::time::Duration;

    #[test]
    fn teardown_slides_only_for_the_steadily_shown_dialog() {
        use ShellSlideKind::{MainMenu, MovieList};
        let start = |running, target, active, entry| {
            exit_start_rule(running, target, active, entry, MovieList)
        };
        let shown = Some(MovieList);
        assert_eq!(start(false, shown, shown, false), ExitStartRule::Slide);
        // Its entry slide still runs, or it is not the showing dialog:
        // 0x00608070 returns without a slide and the result runs at once.
        assert_eq!(start(false, shown, shown, true), ExitStartRule::Immediate);
        assert_eq!(
            start(false, Some(MainMenu), Some(MainMenu), false),
            ExitStartRule::Immediate
        );
        assert_eq!(start(false, None, shown, false), ExitStartRule::Immediate);
        // A second result while it slides out neither restarts nor commits.
        assert_eq!(
            start(true, shown, shown, false),
            ExitStartRule::AlreadyLeaving
        );
    }

    #[test]
    fn exit_results_belong_to_their_dialogs() {
        use crate::ui::main_menu_shell::MainMenuShellAction;
        assert_eq!(
            ShellExitThen::MainMenu(MainMenuShellAction::ExitGame).dialog(),
            ShellSlideKind::MainMenu
        );
        assert_eq!(ShellExitThen::PlayMovie.dialog(), ShellSlideKind::MovieList);
        assert_eq!(
            ShellExitThen::SkirmishBack.dialog(),
            ShellSlideKind::Skirmish
        );
        assert_eq!(
            ShellExitThen::MovieListBack.dialog(),
            ShellSlideKind::MovieList
        );
    }

    /// Last presented tick of a `0xE2` entry at 800x600 (9 tile rows).
    fn main_menu_terminal_tick() -> u8 {
        ShellSlideKind::MainMenu.column(9).total_ticks() as u8 - 1
    }

    #[test]
    fn a_teardown_slide_shows_every_tick_before_its_result() {
        // Movie list 0x129 at 800x600: 9 tile rows, 9 + 9 = 18 ticks of 30 ms.
        let t0 = Instant::now();
        let mut exit = ShellExit {
            wave: ShellFrameWave::new_slide_out(ShellSlideKind::MovieList.column(9), t0),
            then: ShellExitThen::MovieListBack,
            completed_at: None,
        };
        for tick in 1..=17u64 {
            assert!(
                !exit.advance(t0 + Duration::from_millis(30 * tick)),
                "tick {tick}"
            );
        }
        // Every tile row, the buttons and the empty ones, closed on the last tick.
        assert!(exit.wave.button_draws().iter().all(|draw| draw.frame == 10));
        assert!(exit.advance(t0 + Duration::from_millis(30 * 18)));
        assert_eq!(exit.then, ShellExitThen::MovieListBack);
    }

    #[test]
    fn shell_kinds_map_to_their_dialog_ids() {
        assert_eq!(ShellSlideKind::MainMenu.dialog_id(), DialogId(0x00E2));
        assert_eq!(ShellSlideKind::SinglePlayer.dialog_id(), DialogId(0x0100));
        assert_eq!(ShellSlideKind::Skirmish.dialog_id(), DialogId(0x0102));
    }

    #[test]
    fn shell_kinds_resolve_their_slide_columns() {
        let top_and_bottom = |kind: ShellSlideKind| {
            let column = kind.column(9);
            (column.top_buttons, column.bottom_button, column.map_button)
        };
        assert_eq!(top_and_bottom(ShellSlideKind::MainMenu), (5, true, false));
        assert_eq!(
            top_and_bottom(ShellSlideKind::SinglePlayer),
            (3, true, false)
        );
        assert_eq!(
            top_and_bottom(ShellSlideKind::MoviesAndCredits),
            (3, true, false)
        );
        assert_eq!(top_and_bottom(ShellSlideKind::MovieList), (1, true, false));
        assert_eq!(top_and_bottom(ShellSlideKind::Skirmish), (2, true, true));
    }

    #[test]
    fn gsi_13_26_menu_page_first_paint_uses_same_presenter_entrypoint() {
        let source = include_str!("shell_transition.rs");
        let production = source
            .split_once("#[cfg(test)]")
            .expect("test module follows production transition renderer")
            .0;
        let renderer = &production[production
            .find("pub(crate) fn render_shell_first_paint_slide")
            .expect("production first-paint renderer")..];
        let branch = &renderer[renderer
            .find("ShellSlideKind::SinglePlayer | ShellSlideKind::MoviesAndCredits =>")
            .expect("menu-page first-paint branch")..];
        let branch = branch
            .split_once("ShellSlideKind::MainMenu")
            .map_or(branch, |(branch, _)| branch);

        assert!(branch.contains("render_active_menu_page"));
        assert!(branch.contains("destination"));
        assert!(!branch.contains("target"));
    }

    #[test]
    fn collapsed_e2_to_100_back_before_paint_rearms_title_and_entry_wave() {
        let start = Instant::now();
        let mut title_reveal = Kind1StaticReveal::default();
        let mut monitor = crate::ui::shell::warning_monitor::WarningMonitor::default();
        let mut page_title = crate::ui::shell::static_reveal::PresentedKind1Static::new(
            crate::ui::shell::static_reveal::HEADING_KIND1,
        );
        let mut status_line = crate::ui::shell::static_reveal::PresentedKind1Static::new(
            crate::ui::shell::static_reveal::STATUS_LINE_KIND1,
        );
        assert!(title_reveal.start("Main Menu", start));
        for count in 1..=17 {
            let Kind1PaintWindow::Due { window, receipt } = title_reveal.paint_window() else {
                panic!("expected dirty title paint {count}");
            };
            assert_eq!(window.count, count);
            assert!(title_reveal.record_presented(receipt));
            if count < 17 {
                assert!(
                    title_reveal.poll_timer(start + Duration::from_millis(30 * u64::from(count)))
                );
            }
        }
        assert!(title_reveal.is_terminal_persistent());

        let mut active_shell = Some(ShellSlideKind::MainMenu);
        let mut first_paint_slide = None;
        let mut slide_generation = 41;
        let mut slide_sound_edges = Vec::new();

        // Open 0x100: 0xE2 is destroyed, but no 0x100 frame is allowed to run.
        ShellLifecycleReducer {
            panel_rows: 9,
            active_shell: &mut active_shell,
            first_paint_slide: &mut first_paint_slide,
            slide_generation: &mut slide_generation,
            title_reveal: &mut title_reveal,
            monitor: &mut monitor,
            page_title: &mut page_title,
            status_line: &mut status_line,
        }
        .invalidate_main_menu_dialog_instance();
        assert_eq!(title_reveal.paint_window(), Kind1PaintWindow::Hidden);
        assert_eq!(active_shell, None);
        assert!(first_paint_slide.is_none());
        assert!(slide_sound_edges.is_empty());

        // Queued Back destroys 0x100 and recreates 0xE2 before the frame driver.
        ShellLifecycleReducer {
            panel_rows: 9,
            active_shell: &mut active_shell,
            first_paint_slide: &mut first_paint_slide,
            slide_generation: &mut slide_generation,
            title_reveal: &mut title_reveal,
            monitor: &mut monitor,
            page_title: &mut page_title,
            status_line: &mut status_line,
        }
        .invalidate_main_menu_dialog_instance();
        assert!(first_paint_slide.is_none());
        assert!(slide_sound_edges.is_empty());

        // The next production frame observes only the recreated 0xE2. No 0x100
        // wave or sound ever existed; exactly one fresh 0xE2 entry edge does.
        let effect = ShellLifecycleReducer {
            panel_rows: 9,
            active_shell: &mut active_shell,
            first_paint_slide: &mut first_paint_slide,
            slide_generation: &mut slide_generation,
            title_reveal: &mut title_reveal,
            monitor: &mut monitor,
            page_title: &mut page_title,
            status_line: &mut status_line,
        }
        .observe_target(Some(ShellSlideKind::MainMenu), start);
        assert_eq!(effect, ShellEntryEffect::Started(ShellSlideKind::MainMenu));
        assert_eq!(slide_generation, 42);
        assert!(slide_sound_edges.is_empty(), "arming must remain silent");
        assert_eq!(title_reveal.paint_window(), Kind1PaintWindow::Hidden);
        assert!(first_paint_slide.is_some());
        assert_eq!(active_shell, Some(ShellSlideKind::MainMenu));

        // Successful acquisition activates tick 0 and is the one sound edge.
        let wave = first_paint_slide.as_mut().expect("armed main-menu wave");
        assert!(wave.activate_after_acquire());
        slide_sound_edges.push(ShellSlideKind::MainMenu);
        assert_eq!(
            wave.current_main_menu_frame().map(|frame| frame.tick()),
            Some(0)
        );

        let mut accepted_at = start;
        for expected_tick in 0..=main_menu_terminal_tick() {
            let wave = first_paint_slide.as_mut().expect("active main-menu wave");
            let frame = wave.current_main_menu_frame().expect("ready frame");
            assert_eq!((frame.generation(), frame.tick()), (42, expected_tick));
            let token = wave.mint_present_token(frame).expect("matching token");
            wave.record_presented(token, accepted_at).expect("commit");
            if expected_tick < main_menu_terminal_tick() {
                accepted_at += Duration::from_millis(30);
                assert_eq!(
                    wave.poll_presented(accepted_at),
                    Some(PresentedPoll::Acquire)
                );
            }
        }
        let wave = first_paint_slide
            .as_mut()
            .expect("terminal hold remains active");
        assert_eq!(
            wave.poll_presented(accepted_at + Duration::from_millis(29)),
            Some(PresentedPoll::WaitUntil(
                accepted_at + Duration::from_millis(30)
            ))
        );
        assert_eq!(
            wave.poll_presented(accepted_at + Duration::from_millis(30)),
            Some(PresentedPoll::Complete)
        );
        assert!(first_paint_slide.is_some(), "completing still blocks input");
        assert_eq!(title_reveal.paint_window(), Kind1PaintWindow::Hidden);

        let completion_at = accepted_at + Duration::from_millis(30);
        let mut completion_events = vec!["ShellButtonSlideSound"];
        assert!(
            ShellLifecycleReducer {
                panel_rows: 9,
                active_shell: &mut active_shell,
                first_paint_slide: &mut first_paint_slide,
                slide_generation: &mut slide_generation,
                title_reveal: &mut title_reveal,
                monitor: &mut monitor,
                page_title: &mut page_title,
                status_line: &mut status_line,
            }
            .complete_presented_main_menu(42, "Main Menu", completion_at)
        );
        completion_events.push("title-start");
        completion_events.push("clear");
        assert_eq!(
            completion_events,
            ["ShellButtonSlideSound", "title-start", "clear"]
        );
        assert!(first_paint_slide.is_none());
        let Kind1PaintWindow::Due { window, .. } = title_reveal.paint_window() else {
            panic!("recreated title did not begin a fresh reveal");
        };
        assert_eq!(window.count, 1);
        assert_eq!(slide_sound_edges, [ShellSlideKind::MainMenu]);
        // The same SHOW completion starts status line 0x695 (0x0060AA60).
        assert!(status_line.paint(completion_at).is_some());
    }

    #[test]
    fn a_new_dialog_instance_hides_its_statics_and_restarts_the_monitor() {
        use crate::ui::shell::static_reveal::{PresentedKind1Static, STATUS_LINE_KIND1};
        use crate::ui::shell::warning_monitor::WarningMonitor;
        let t0 = Instant::now();
        let mut title_reveal = Kind1StaticReveal::default();
        let mut monitor = WarningMonitor::default();
        let mut page_title =
            PresentedKind1Static::new(crate::ui::shell::static_reveal::HEADING_KIND1);
        let mut status_line = PresentedKind1Static::new(STATUS_LINE_KIND1);
        // A finished Single Player instance: statics shown, monitor animating.
        page_title.set_text("Single Player", t0);
        assert!(page_title.start(t0));
        status_line.set_text("Help", t0);
        assert!(status_line.start(t0));
        for ms in [2, 100, 200] {
            monitor.paint(t0 + Duration::from_millis(ms), 91, true);
            monitor.commit_presented();
        }
        assert_ne!(
            monitor.paint(t0 + Duration::from_millis(300), 91, true),
            Some(0)
        );

        let mut active_shell = Some(ShellSlideKind::SinglePlayer);
        let mut first_paint_slide = None;
        let mut slide_generation = 0;
        let later = t0 + Duration::from_secs(1);
        let effect = ShellLifecycleReducer {
            panel_rows: 9,
            active_shell: &mut active_shell,
            first_paint_slide: &mut first_paint_slide,
            slide_generation: &mut slide_generation,
            title_reveal: &mut title_reveal,
            monitor: &mut monitor,
            page_title: &mut page_title,
            status_line: &mut status_line,
        }
        .observe_target(Some(ShellSlideKind::MoviesAndCredits), later);
        assert_eq!(
            effect,
            ShellEntryEffect::Started(ShellSlideKind::MoviesAndCredits)
        );
        assert_eq!(page_title.paint(later), None);
        assert_eq!(status_line.paint(later), None);
        assert_eq!(monitor.paint(later, 91, false), Some(0));
    }

    #[test]
    fn completion_generation_mismatch_poison_prevents_retry() {
        let start = Instant::now();
        let mut active_shell = Some(ShellSlideKind::MainMenu);
        let mut first_paint_slide = Some(ShellFrameWave::new_presented_main_menu(
            52,
            ShellSlideKind::MainMenu.column(9),
        ));
        let mut slide_generation = 52;
        let mut title_reveal = Kind1StaticReveal::default();
        let mut monitor = crate::ui::shell::warning_monitor::WarningMonitor::default();
        let mut page_title = crate::ui::shell::static_reveal::PresentedKind1Static::new(
            crate::ui::shell::static_reveal::HEADING_KIND1,
        );
        let mut status_line = crate::ui::shell::static_reveal::PresentedKind1Static::new(
            crate::ui::shell::static_reveal::STATUS_LINE_KIND1,
        );

        let wave = first_paint_slide.as_mut().expect("presented wave");
        assert!(wave.activate_after_acquire());
        let mut accepted_at = start;
        for expected_tick in 0..=main_menu_terminal_tick() {
            let frame = wave.current_main_menu_frame().expect("ready frame");
            assert_eq!(frame.tick(), expected_tick);
            let token = wave.mint_present_token(frame).expect("matching token");
            wave.record_presented(token, accepted_at).expect("commit");
            accepted_at += Duration::from_millis(30);
            let expected_poll = if expected_tick < main_menu_terminal_tick() {
                PresentedPoll::Acquire
            } else {
                PresentedPoll::Complete
            };
            assert_eq!(wave.poll_presented(accepted_at), Some(expected_poll));
        }

        assert!(
            !ShellLifecycleReducer {
                panel_rows: 9,
                active_shell: &mut active_shell,
                first_paint_slide: &mut first_paint_slide,
                slide_generation: &mut slide_generation,
                title_reveal: &mut title_reveal,
                monitor: &mut monitor,
                page_title: &mut page_title,
                status_line: &mut status_line,
            }
            .complete_presented_main_menu(51, "Main Menu", accepted_at)
        );
        let wave = first_paint_slide.as_mut().expect("poisoned wave remains");
        assert!(wave.is_presented_poisoned());
        assert_eq!(
            wave.poll_presented(accepted_at),
            Some(PresentedPoll::Poisoned),
            "a failed completion transaction must never replay its hook"
        );
        assert_eq!(title_reveal.paint_window(), Kind1PaintWindow::Hidden);
    }
}
