//! The shell slide driver (menu / single-player / skirmish / score pages).
//!
//! The original slides every allow-listed shell dialog in on its first paint and
//! out when it closes — not a menu->skirmish edge transition and not a
//! whole-screen crossfade. The slide engine animates the dialog's right-panel
//! column (every tile row's SDBTNANM frame, plus the Skirmish map button and top
//! panel) on a 30 ms tick; nothing is repositioned.
//!
//! The render-agnostic data + schedule live in [`crate::ui::shell::slide`] (each
//! rendered dialog's slide column and the [`ShellFrameWave`] frame sweep). This
//! module is the one driver of both slides: before each draw it commits a
//! finished slide-out, notices a newly shown dialog, (re)starts and advances its
//! entry wave and runs the dialog's SHOW completion (its statics) when the wave
//! ends. The page renderers only read the waves. The slide-in start cue is
//! `GUIMoveInSound` (stock `MenuSlideIn`); the end cue `ShellButtonSlideSound` is
//! empty in stock rules.

use crate::util::time::{Duration, Instant};

use crate::app::AppState;
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::menu_page::MenuPageSpec;

pub(crate) use crate::ui::shell::slide::{
    ColumnDraw, PanelArt, ShellFrameWave, SlideColumn, WaveDirection,
};

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
    /// VERA's Mods page, from the Mods button of Options.
    Mods,
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
    /// Dialog 0x10F — Westwood Online login.
    WolLogin,
    /// Dialog 0x113 — Westwood Online custom-match lobby.
    WolLobby,
    /// Dialog 0x2BC — Westwood Online Options, over the hidden lobby.
    WolOptions,
    /// Dialog 0xD6 — the shell's Sound Options, run from WOL Options.
    WolSoundOptions,
    /// Dialog 0x112 — the WOL auto-login query after a dialog login.
    WolAutoLoginQuery,
    /// Dialog 0xFE — the WOL locale page during the connect.
    WolLocale,
    /// Dialog 0x114 — WOL My Information.
    WolMyInformation,
    /// Dialog 0x116 — the WOL Buddy List.
    WolBuddyList,
    /// Dialog 0x117 — WOL Quick Match's status page.
    WolQuickMatch,
    /// Dialog 0x122 — WOL Quick Match's postmatch add-buddy query.
    WolAddBuddyQuery,
    /// Dialog 0xA3 — Options' Keyboard page (front-end parent only).
    Keyboard,
    LanLobby,
    /// BC/BD (LAN) or C2/C9 (WOL) network setup.
    NetworkSetup(crate::ui::multiplayer_setup::SetupDialog),
    /// Dialog 0x6B — Skirmish's Choose Map, a family page of its own.
    ChooseMap,
    /// Dialog 0x108 — the score screen after a skirmish game.
    Score,
    /// Dialog 0x103 — campaign victory StandaloneScore.
    CampaignScore,
    /// Dialog 0xBC7 — the LAN Cooperative campaign end.
    CampaignEnd,
    /// Dialog 0x105 — Choose Map's random-map dialog.
    RandomMap,
}

impl ShellSlideKind {
    /// The shell of a campaign score page's dialog.
    pub(crate) fn campaign_score(
        dialog: crate::ui::campaign_score_shell::CampaignScoreDialog,
    ) -> Self {
        match dialog {
            crate::ui::campaign_score_shell::CampaignScoreDialog::Victory => Self::CampaignScore,
            crate::ui::campaign_score_shell::CampaignScoreDialog::CampaignEnd => Self::CampaignEnd,
        }
    }

    /// The Win32 dialog resource id this shell maps to. The slide's column is
    /// looked up from this id in the data-driven `slide` table.
    pub(crate) fn dialog_id(self) -> DialogResourceId {
        DialogResourceId(match self {
            ShellSlideKind::MainMenu => 0x00E2,
            ShellSlideKind::SinglePlayer => 0x0100,
            ShellSlideKind::MoviesAndCredits => 0x0101,
            ShellSlideKind::MovieList => 0x0129,
            ShellSlideKind::Mods => crate::ui::mods_shell::DIALOG.0,
            ShellSlideKind::Skirmish => 0x0102,
            ShellSlideKind::Campaign => 0x0094,
            ShellSlideKind::LoadSavedGame => 0x00B7,
            ShellSlideKind::Options => 0x00D5,
            ShellSlideKind::WolWelcome => 0x010E,
            ShellSlideKind::WolLogin => 0x010F,
            ShellSlideKind::WolLobby => 0x0113,
            ShellSlideKind::WolOptions => 0x02BC,
            ShellSlideKind::WolSoundOptions => 0x00D6,
            ShellSlideKind::WolAutoLoginQuery => 0x0112,
            ShellSlideKind::WolLocale => 0x00FE,
            ShellSlideKind::WolMyInformation => 0x0114,
            ShellSlideKind::WolBuddyList => 0x0116,
            ShellSlideKind::WolQuickMatch => 0x0117,
            ShellSlideKind::WolAddBuddyQuery => 0x0122,
            ShellSlideKind::Keyboard => 0x00A3,
            ShellSlideKind::LanLobby => 0xBB,
            ShellSlideKind::NetworkSetup(dialog) => dialog.page().dialog.0,
            ShellSlideKind::ChooseMap => 0x006B,
            ShellSlideKind::Score => 0x0108,
            ShellSlideKind::CampaignScore => 0x0103,
            ShellSlideKind::CampaignEnd => 0x0BC7,
            ShellSlideKind::RandomMap => 0x0105,
        })
    }

    /// The dialog's slide column with the panel's `rows` tile rows. Every
    /// rendered shell has a `slide` table entry, so a miss is a programming
    /// error. Options slides VERA's page buttons and its Mods button under
    /// retail's Keyboard.
    /// Skirmish slides VERA's Game Options between Start Game and Choose Map,
    /// and VERA's Setups under Choose Map.
    pub(crate) fn column(self, rows: u32, page: Option<&MenuPageSpec>) -> SlideColumn {
        let spec = crate::ui::shell::slide::slide_spec_for(self.dialog_id())
            .expect("rendered shell dialog must have a slide column");
        let page = page.filter(|page| page.dialog == self.dialog_id());
        // WOL's simplified pages must slide only their visible buttons too.
        let top_buttons = page.map_or_else(
            || match self {
                ShellSlideKind::Options => {
                    crate::ui::main_menu_dialogs::options::shell::OPTIONS_PAGE
                        .stacked
                        .len() as u32
                }
                ShellSlideKind::Skirmish => spec.top_buttons + 2,
                _ => spec.top_buttons,
            },
            |page| page.stacked.len() as u32,
        );
        crate::ui::shell::slide::SlideDialogSpec {
            top_buttons,
            // The online lobby keeps the warning monitor for its top panel.
            top_panel: spec.top_panel && page.is_none_or(crate::ui::wol_lobby::keeps_top_panel),
            ..spec
        }
        .column(rows)
    }
}

/// A new wave of `kind` in `direction`, on the right-panel tile rows of the
/// current surface (`[0x00B0FA20]`).
fn new_wave(state: &AppState, kind: ShellSlideKind, direction: WaveDirection) -> ShellFrameWave {
    // A fitted shell paints the scaled 800x600 column, so its slide must use
    // the authored 9 tile rows even when the live window is too short for
    // `right_panel_rects(window)` to reach the same count. Retail 640/800
    // keep the live window's own rows, but for Options where they are too
    // few for its column: that page is fitted there too.
    let config = &state.renderer.gpu.config;
    let (width, height) = (config.width as i32, config.height as i32);
    let fitted = crate::app::frontend::shell_layout::fills_window(state)
        || (kind == ShellSlideKind::Options
            && !crate::ui::main_menu_dialogs::options::shell::retail_column_fits(width, height));
    let (width, height) = if fitted {
        (
            crate::ui::shell::geom::SHELL_BASE_W,
            crate::ui::shell::geom::SHELL_BASE_H,
        )
    } else {
        (width, height)
    };
    let rows = crate::ui::shell::geom::right_panel_rects(width, height)
        .tile_count
        .max(1) as u32;
    let page = state.frontend.wol.as_ref().and_then(|visit| match kind {
        ShellSlideKind::WolWelcome => Some(&crate::ui::wol_shell::WOL_WELCOME_PAGE),
        ShellSlideKind::WolLogin => Some(visit.login()?.policy.page_spec()),
        ShellSlideKind::WolLobby => Some(visit.lobby()?.ui.page_spec()),
        _ => None,
    });
    let mut column = kind.column(rows, page);
    if kind == ShellSlideKind::Options
        && let Some(dialog) = state.frontend.options_dialog.as_ref()
        && let Some((top_buttons, button_above_back)) =
            crate::ui::main_menu_dialogs::options::shell::launcher_slide_column(dialog.category())
    {
        column.top_buttons = top_buttons;
        column.button_above_back = button_above_back;
    }
    ShellFrameWave::new(column, direction, Instant::now())
}

/// What a family dialog's teardown leads to once its slide-out has run: the
/// dialog proc's result, dispatched to the state that owns it.
pub(crate) type ShellExitThen = Box<dyn FnOnce(&mut AppState)>;

/// A shown family dialog's teardown slide (`0x00622720 -> 0x00608070`): the
/// buttons ramp out on the entry schedule while input is blocked, then the
/// dialog is destroyed and `then` runs.
pub(crate) struct ShellExit {
    wave: ShellFrameWave,
    /// The dialog whose result this is.
    kind: ShellSlideKind,
    /// The result is an agreed network start (the only one that commits while
    /// the room is covered).
    network_start: bool,
    then: ShellExitThen,
    /// When the last tick had been shown.
    completed_at: Option<Instant>,
}

impl std::fmt::Debug for ShellExit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShellExit")
            .field("kind", &self.kind)
            .field("network_start", &self.network_start)
            .finish_non_exhaustive()
    }
}

/// After tearing `0x94` down, state 8 waits while the campaign voice still
/// plays, at most this long (`0x0052E036..0x0052E089`, `0xBB8` ms).
const CAMPAIGN_VOICE_WAIT: Duration = Duration::from_millis(3000);

/// How a request to leave a family dialog starts.
pub(crate) enum ShellExitStart {
    /// The teardown slide runs; the result follows when it ends.
    Sliding,
    /// The dialog is not showing steady, so `0x00608070` returns without a
    /// slide: the returned result runs at once.
    Immediate(ShellExitThen),
    /// The dialog is already sliding out with a result; this one is dropped.
    AlreadyLeaving,
}

/// Begin the teardown slide of dialog `kind`, which produced `then`. Only the
/// steadily shown dialog slides out: one still sliding in, or one that is not
/// the showing dialog, returns its result at once.
pub(crate) fn begin_shell_exit(
    state: &mut AppState,
    kind: ShellSlideKind,
    network_start: bool,
    then: ShellExitThen,
) -> ShellExitStart {
    if state.frontend.shell_exit.is_some() {
        return ShellExitStart::AlreadyLeaving;
    }
    if current_shell_slide_target(state) != Some(kind)
        || state.frontend.shell_slide_active_shell != Some(kind)
        || state.frontend.shell_first_paint_slide.is_some()
        || exits_in_place(kind)
    {
        return ShellExitStart::Immediate(then);
    }
    state.frontend.shell_exit = Some(ShellExit {
        wave: new_wave(state, kind, WaveDirection::SlideOut),
        kind,
        network_start,
        then,
        completed_at: None,
    });
    ShellExitStart::Sliding
}

impl ShellExit {
    /// Advance one 30 ms tick when due; true once every tick has been shown
    /// (`0x006071E0` draws ticks `0..bound`, then the teardown continues).
    fn advance(&mut self, now: Instant, hidden_online_start: bool) -> bool {
        if hidden_online_start {
            // The background admission loop only commits an agreed network
            // start. Other dialog transitions wait for their ordinary paint.
            return self.network_start;
        }
        self.wave.advance(now);
        self.wave.is_complete()
    }
}

/// Advance the running teardown slide; returns its result once the last tick
/// has been shown. A dialog that stopped showing while it slid out (another
/// route replaced it, such as a game loaded from an overlay panel) takes its
/// result with it.
pub(crate) fn advance_shell_exit(state: &mut AppState, now: Instant) -> Option<ShellExitThen> {
    let kind = state.frontend.shell_exit.as_ref()?.kind;
    if current_shell_slide_target(state) != Some(kind) {
        state.frontend.shell_exit = None;
        return None;
    }
    let hidden_online_start = crate::app::App::online_start_runs_while_hidden(state);
    let exit = state.frontend.shell_exit.as_mut()?;
    if !exit.advance(now, hidden_online_start) {
        return None;
    }
    if kind == ShellSlideKind::Campaign {
        // The last slide-out frame stays on screen while the voice plays.
        let completed_at = *exit.completed_at.get_or_insert(now);
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
        .filter(|exit| exit.kind == kind)
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

/// Whether `to` opens in place of `from`, as a page of VERA's Options window
/// replaces another: Options `0xD5` and its Keyboard page `0xA3`, in either
/// direction ([`exits_in_place`] has the matching exits). Retail slides
/// `0xD5` out before `0xA3` (`0x0055FD06`) and `0xA3` out before the new
/// `0xD5` (`0x005FBEF0`); VERA's Options window is its own layout, so such
/// an entry arms no slide and plays no cue, and the page's statics start at
/// once as a slide's SHOW completion would start them.
fn moves_in_place(from: Option<ShellSlideKind>, to: ShellSlideKind) -> bool {
    matches!(
        (from, to),
        (Some(ShellSlideKind::Options), ShellSlideKind::Keyboard)
            | (Some(ShellSlideKind::Keyboard), ShellSlideKind::Options)
    )
}

/// Whether leaving `kind` opens its next dialog in place ([`moves_in_place`]):
/// the front-end Keyboard page, whose Back and Cancel both rebuild Options.
/// Options' own Keyboard button commits at once from its owner
/// (`App::commit_launcher_options_result`).
fn exits_in_place(kind: ShellSlideKind) -> bool {
    kind == ShellSlideKind::Keyboard
}

/// Forget the destroyed/recreated 0xE2 dialog instance at an actual route
/// boundary, even when the destination never reaches a paint, so a collapsed
/// 0xE2 -> 0x100 -> Back round trip makes a fresh 0xE2 entry edge (new
/// statics, new slide) instead of inheriting the old terminal title.
pub(crate) fn invalidate_main_menu_dialog_instance(state: &mut AppState) {
    state.frontend.shell_slide_active_shell = None;
    state.frontend.shell_first_paint_slide = None;
    // A new 0xE2 starts with no press or hover: its status line stays empty
    // until a hover message arrives from the real cursor position. The
    // controller's 0xE2 instance is cleared wherever it sits, under a message
    // box included.
    state
        .frontend
        .shell_controller
        .restart_instance(crate::ui::main_menu_shell::MAIN_MENU_DIALOG);
}

/// Drive the entry slide before the frame is drawn: a newly showing dialog
/// gets new statics and starts its slide-in with its cue; a running slide-in
/// advances one 30 ms tick when due; once every tick has been shown the
/// dialog's SHOW completion runs, so this frame already paints the steady
/// dialog. Returns the dialog whose slide-in completed.
pub(crate) fn drive_shell_entry(state: &mut AppState, now: Instant) -> Option<ShellSlideKind> {
    let target = current_shell_slide_target(state);
    let previous = state.frontend.shell_slide_active_shell;
    if target != previous {
        state.frontend.shell_slide_active_shell = target;
        state.frontend.shell_first_paint_slide = None;
        if let Some(kind) = target {
            // A new dialog instance creates new statics: 0x71C at frame 0
            // with its timer unarmed (0x0060A982), hidden 0x694 and 0x695.
            let frontend = &mut state.frontend;
            frontend.shell_monitor = Default::default();
            frontend.shell_page_title.reset();
            frontend.shell_status_line.reset();
            // A page that opens in place of the other arms no slide: its
            // statics start now, as the SHOW completion would start them.
            if moves_in_place(previous, kind) {
                show_completion(state, kind, now);
                return Some(kind);
            }
            let wave = new_wave(state, kind, WaveDirection::SlideIn);
            state.frontend.shell_first_paint_slide = Some(wave);
            crate::app::App::play_shell_slide_in_sound(state);
        }
    }
    let kind = target?;
    let wave = state.frontend.shell_first_paint_slide.as_mut()?;
    wave.advance(now);
    if !wave.is_complete() {
        return None;
    }
    state.frontend.shell_first_paint_slide = None;
    // The slide-in's end cue precedes its SHOW completion (`0x00607F59`).
    crate::app::App::play_shell_slide_completion_sound(state);
    show_completion(state, kind, now);
    Some(kind)
}

/// The dialog's SHOW completion (`0x4EC -> 0x0060AA60`): its kind-1 statics
/// start (or repaint, for a dialog shown again).
fn show_completion(state: &mut AppState, kind: ShellSlideKind, now: Instant) {
    use crate::app::frontend::menu_page_render::active_page_title_text;
    let heading = match kind {
        // `0x102`'s own statics start, or repaint when the dialog shows again
        // after Choose Map.
        ShellSlideKind::NetworkSetup(_) => {
            return crate::app::shell_network_setup::show_statics(state);
        }
        ShellSlideKind::Skirmish => {
            let (title, game_type, map_label) =
                crate::app::frontend::skirmish_shell_render::skirmish_right_panel_label_strings(
                    state,
                );
            let statics = &mut state.frontend.skirmish_shell_state.statics;
            return statics.show(&title, &game_type, &map_label, now);
        }
        // Choose Map `0x6B`, whose statics outlive the random-map dialog, and
        // `0x105`, whose statics outlive the seed browser.
        ShellSlideKind::ChooseMap | ShellSlideKind::RandomMap => {
            let title = active_page_title_text(state, kind);
            let shell = &mut state.frontend.skirmish_shell_state;
            let statics = if kind == ShellSlideKind::ChooseMap {
                shell
                    .choose_map_modal
                    .as_mut()
                    .map(|modal| &mut modal.statics)
            } else {
                shell
                    .random_map_setup_modal
                    .as_mut()
                    .map(|setup| &mut setup.statics)
            };
            if let Some(statics) = statics {
                statics.show(&title, now);
            }
            return;
        }
        // 0x103's SHOW completion (`0x4EC` on the dialog) starts every kind-1
        // static and, after playing ScoreAnimSound, the 0x72B kind-4 timer.
        // 0xBC7's (`0x00470D30`) plays no sound.
        ShellSlideKind::CampaignScore | ShellSlideKind::CampaignEnd => {
            let frames = state
                .frontend
                .campaign_score_art
                .as_ref()
                .and_then(|art| art.scra.as_ref())
                .map_or(0, |strip| strip.frames.len());
            let sound = state
                .rules()
                .filter(|_| kind == ShellSlideKind::CampaignScore)
                .and_then(|rules| rules.general.score_anim_sound())
                .map(str::to_owned);
            if let Some(page) = state.frontend.campaign_score_page.as_mut() {
                page.start_presentation(frames, now);
            }
            if let Some(sound) = sound {
                crate::app::App::play_campaign_score_anim_sound(state, Some(&sound));
            }
            return;
        }
        // 0x108's SHOW completion starts every kind-1 static at once: the
        // heading, the status line and the 47 table texts.
        ShellSlideKind::Score => {
            if let Some(page) = state.frontend.score_page.as_mut() {
                page.statics.start(now);
            }
            crate::app::frontend::score_shell_render::score_title_text(state)
        }
        ShellSlideKind::MainMenu => {
            crate::app::frontend::main_menu_shell_render::main_menu_title_text(state).into_owned()
        }
        _ => active_page_title_text(state, kind),
    };
    // The family pages' heading 0x694 and status line 0x695.
    let frontend = &mut state.frontend;
    frontend.shell_page_title.set_text(&heading, now);
    frontend.shell_page_title.start(now);
    frontend.shell_status_line.start(now);
}

pub(crate) fn blocks_shell_input(state: &AppState) -> bool {
    // The graceful quit cascade also freezes shell input (the original processes
    // no input during its blocking teardown), so a stray click can't re-enter the
    // menu mid-fade.
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    if state.frontend.browser_exit_pending {
        return true;
    }
    state.frontend.quit_cascade.is_some()
        || shell_slide_running(state)
        || external_menu_wait(state).is_some()
}

/// Why an external network launch (docs/spawn.md) is at the main menu. It
/// shows the empty menu backdrop there and takes no shell input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExternalMenuWait {
    /// Admission, before the match first reaches the screen.
    Joining,
    /// The match ran; the launch is reporting it.
    Reporting,
}

pub(crate) fn external_menu_wait(state: &AppState) -> Option<ExternalMenuWait> {
    let external = state.frontend.screen == crate::ui::game_screen::GameScreen::MainMenu
        && (crate::app::shell_wol::spawned_visit(state)
            || crate::app::shell_network_setup::external(state));
    external.then(|| {
        if state.frontend.external_progress.match_started.is_none() {
            ExternalMenuWait::Joining
        } else {
            ExternalMenuWait::Reporting
        }
    })
}

/// Which allow-listed shell dialog is currently showing, if any. Mirrors the
/// main-menu render dispatch order (skirmish > single-player > bare menu); the
/// Utility-card / skirmish-setup paths are not native shell dialogs and do not
/// slide. Returns the score dialog on the result screen and `None` off the
/// shell screens.
pub(crate) fn current_shell_slide_target(state: &AppState) -> Option<ShellSlideKind> {
    use crate::ui::game_screen::GameScreen;
    // The score dialog runs after the game, before the shell resumes.
    if matches!(state.frontend.screen, GameScreen::MissionResult { .. }) {
        if crate::app::App::score_shell_active(state) {
            return Some(ShellSlideKind::Score);
        }
        if crate::app::App::campaign_score_shell_active(state) {
            return Some(ShellSlideKind::campaign_score(
                state
                    .frontend
                    .campaign_score_page
                    .as_ref()
                    .map_or_else(Default::default, |page| page.dialog),
            ));
        }
        return None;
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
    // (state 7) run without a family dialog. VERA's Esc menu asks over the
    // showing dialog, which stays the target.
    if let Some(dialog) = state.frontend.keyboard_dialog.as_ref() {
        return (dialog.parent == crate::ui::shell::keyboard::KeyboardParent::Launcher)
            .then_some(ShellSlideKind::Keyboard);
    }
    if state
        .frontend
        .shell_message_box
        .as_ref()
        .is_some_and(|message_box| !message_box.keeps_page())
        || state.frontend.quit_cascade.is_some()
    {
        return None;
    }
    // Only the native page slides; the assetless fallback does not.
    if state.frontend.options_dialog.is_some() {
        return crate::app::App::native_launcher_options_active(state)
            .then_some(ShellSlideKind::Options);
    }
    use crate::app::shell_route::ShellRoute;
    use crate::app::shell_wol::WolPage;
    let shell = &state.frontend.skirmish_shell_state;
    let wol_page = state.frontend.wol.as_ref().map(|visit| &visit.page);
    Some(match state.frontend.shell_route {
        ShellRoute::Skirmish => skirmish_slide_target(shell)?,
        ShellRoute::LanLobby => ShellSlideKind::LanLobby,
        ShellRoute::NetworkSetup if shell.choose_map_modal.is_some() => {
            skirmish_slide_target(shell)?
        }
        ShellRoute::NetworkSetup => {
            ShellSlideKind::NetworkSetup(crate::app::shell_network_setup::dialog(state))
        }
        ShellRoute::SinglePlayer => ShellSlideKind::SinglePlayer,
        ShellRoute::MoviesAndCredits => ShellSlideKind::MoviesAndCredits,
        ShellRoute::MovieList => ShellSlideKind::MovieList,
        ShellRoute::Mods => ShellSlideKind::Mods,
        ShellRoute::Campaign => ShellSlideKind::Campaign,
        ShellRoute::LoadSavedGame => ShellSlideKind::LoadSavedGame,
        // The waits and boxes run after a WOL dialog is destroyed, over the
        // empty backdrop.
        ShellRoute::Wol => match wol_page? {
            WolPage::Welcome => ShellSlideKind::WolWelcome,
            WolPage::Login(_) => ShellSlideKind::WolLogin,
            WolPage::AutoLoginQuery => ShellSlideKind::WolAutoLoginQuery,
            WolPage::Locale(_) => ShellSlideKind::WolLocale,
            WolPage::MyInformation(_) => ShellSlideKind::WolMyInformation,
            WolPage::Buddy(_) => ShellSlideKind::WolBuddyList,
            WolPage::QuickMatch(_) => ShellSlideKind::WolQuickMatch,
            WolPage::AddBuddyQuery(_) => ShellSlideKind::WolAddBuddyQuery,
            _ => return None,
        },
        // 0x2BC and its 0xD6 run over the hidden lobby.
        ShellRoute::WolLobby => match wol_page {
            Some(WolPage::Options(page)) if page.sound.is_some() => ShellSlideKind::WolSoundOptions,
            Some(WolPage::Options(_)) => ShellSlideKind::WolOptions,
            // 0x113 is created hidden and hidden again for a game join.
            _ if !state
                .frontend
                .wol
                .as_ref()
                .and_then(|visit| visit.lobby())
                .is_some_and(|lobby| lobby.shown) =>
            {
                return None;
            }
            _ => ShellSlideKind::WolLobby,
        },
        ShellRoute::MainMenu if state.frontend.main_menu_shell_error.is_none() => {
            ShellSlideKind::MainMenu
        }
        ShellRoute::MainMenu => return None,
    })
}

/// The shown dialog of the Skirmish stack. `0x105` stays shown under the
/// seed browser, so closing the browser uncovers it without an entry slide.
pub(crate) fn skirmish_slide_target(
    shell: &crate::ui::skirmish_shell::SkirmishShellState,
) -> Option<ShellSlideKind> {
    use crate::ui::skirmish_shell::SkirmishShellDialog;
    Some(match shell.top_dialog()? {
        // VERA's Setups page covers `0x102` without a slide of its own.
        SkirmishShellDialog::Skirmish | SkirmishShellDialog::Setups => ShellSlideKind::Skirmish,
        SkirmishShellDialog::ChooseMap => ShellSlideKind::ChooseMap,
        SkirmishShellDialog::RandomMap | SkirmishShellDialog::SeedBrowser => {
            ShellSlideKind::RandomMap
        }
    })
}

#[cfg(all(test, tests_app_frontend))]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Options and its Keyboard page replace each other in place, as the
    /// pages of VERA's Options window do, while every other way into either
    /// still slides.
    #[test]
    fn options_and_its_keyboard_page_replace_each_other_without_a_slide() {
        use ShellSlideKind::{Keyboard, MainMenu, Options};
        assert!(moves_in_place(Some(Options), Keyboard));
        assert!(moves_in_place(Some(Keyboard), Options));
        assert!(!moves_in_place(Some(MainMenu), Options));
        assert!(!moves_in_place(None, Keyboard));
        assert!(exits_in_place(Keyboard));
        assert!(!exits_in_place(Options));
    }

    #[test]
    fn campaign_score_owns_dialog_0x103() {
        assert_eq!(
            ShellSlideKind::CampaignScore.dialog_id(),
            DialogResourceId(0x0103)
        );
        assert!(crate::ui::shell::slide::slide_spec_for(DialogResourceId(0x0103)).is_some());
    }

    #[test]
    fn the_skirmish_stack_slides_only_the_dialog_that_shows() {
        use crate::ui::skirmish_shell::{
            ChooseMapModalState, ChooseMapPrompt, ChooseMapSelection, RandomMapSetupModalState,
            SavedSeedBrowserState, SavedSeedMode, SkirmishShellState,
            compute_choose_map_modal_layout,
        };
        let mut shell = SkirmishShellState::default();
        assert_eq!(
            skirmish_slide_target(&shell),
            Some(ShellSlideKind::Skirmish)
        );
        let chooser = compute_choose_map_modal_layout(800, 600);
        shell.choose_map_modal = Some(ChooseMapModalState::open(
            crate::ui::skirmish_shell::ChooseMapContext::Skirmish,
            1,
            None,
            &[],
            &[],
            &chooser,
        ));
        assert_eq!(
            skirmish_slide_target(&shell),
            Some(ShellSlideKind::ChooseMap)
        );
        // Create Random Map hides the chooser while `0x105` runs.
        shell.choose_map_modal.as_mut().unwrap().hide();
        let options = crate::map::rmg::options::RmgOptions {
            seed: 1,
            ..Default::default()
        };
        shell.random_map_setup_modal = Some(RandomMapSetupModalState::open(options, None, false));
        assert_eq!(
            skirmish_slide_target(&shell),
            Some(ShellSlideKind::RandomMap)
        );
        // Load, Save and Delete Map run the browser over `0x105` without
        // hiding it (`0x0059693F` -> `0x00558DD0`): no target change, so
        // closing the browser starts no entry slide.
        shell.saved_seed_browser = Some(SavedSeedBrowserState::open(
            SavedSeedMode::Load,
            Vec::new(),
            "".into(),
            "".into(),
            0,
            1,
        ));
        assert_eq!(
            skirmish_slide_target(&shell),
            Some(ShellSlideKind::RandomMap)
        );
        shell.saved_seed_browser = None;
        // Use Map closes `0x105`, and the hidden chooser's Use Map asks to
        // eject AI players (`0x005E6B2F`): no dialog shows, so none slides,
        // and OK closes the chooser at once.
        shell.random_map_setup_modal = None;
        let chooser = shell.choose_map_modal.as_mut().unwrap();
        chooser.message_box = Some(ChooseMapPrompt {
            selection: ChooseMapSelection {
                mode: 1.into(),
                record_index: Some(0),
            },
            kind: crate::ui::skirmish_shell::ChooseMapPromptKind::EjectAi,
        });
        assert_eq!(skirmish_slide_target(&shell), None);
        // Cancel shows the chooser again; its first paint slides it in
        // (`0x005E6B47`, `0x005E6B51`).
        assert!(shell.choose_map_modal.as_mut().unwrap().close_prompt());
        assert_eq!(
            skirmish_slide_target(&shell),
            Some(ShellSlideKind::ChooseMap)
        );
    }

    #[test]
    fn a_covered_room_can_commit_its_network_start_without_a_presented_slide() {
        let now = Instant::now();
        let dialog = crate::ui::multiplayer_setup::SetupDialog {
            host: false,
            flavor: crate::ui::multiplayer_setup::SetupFlavor::Wol,
            modern: true,
        };
        let mut exit = ShellExit {
            wave: ShellFrameWave::new(
                ShellSlideKind::NetworkSetup(dialog).column(9, None),
                WaveDirection::SlideOut,
                now,
            ),
            kind: ShellSlideKind::NetworkSetup(dialog),
            network_start: true,
            then: Box::new(|_| {}),
            completed_at: None,
        };
        assert!(!exit.advance(now, false));
        assert!(exit.advance(now, true));
        exit.network_start = false;
        assert!(!exit.advance(now, true));
    }

    #[test]
    fn a_teardown_slide_shows_every_tick_before_its_result() {
        // Movie list 0x129 at 800x600: 9 tile rows, 9 + 9 = 18 ticks of 30 ms.
        let t0 = Instant::now();
        let mut exit = ShellExit {
            wave: ShellFrameWave::new(
                ShellSlideKind::MovieList.column(9, None),
                WaveDirection::SlideOut,
                t0,
            ),
            kind: ShellSlideKind::MovieList,
            network_start: false,
            then: Box::new(|_| {}),
            completed_at: None,
        };
        for tick in 1..=17u64 {
            assert!(
                !exit.advance(t0 + Duration::from_millis(30 * tick), false),
                "tick {tick}"
            );
        }
        // Every tile row, the buttons and the empty ones, closed on the last tick.
        assert!(exit.wave.button_draws().iter().all(|draw| draw.frame == 10));
        assert!(exit.advance(t0 + Duration::from_millis(30 * 18), false));
    }

    #[test]
    fn shell_kinds_resolve_their_slide_columns() {
        let top_and_bottom = |kind: ShellSlideKind| {
            let column = kind.column(9, None);
            (column.top_buttons, column.bottom_button, column.map_button)
        };
        assert_eq!(top_and_bottom(ShellSlideKind::MainMenu), (5, true, false));
        assert_eq!(
            top_and_bottom(ShellSlideKind::Mods),
            (
                crate::ui::mods_shell::PAGE.stacked.len() as u32,
                true,
                false
            )
        );
        assert_eq!(
            top_and_bottom(ShellSlideKind::SinglePlayer),
            (3, true, false)
        );
        assert_eq!(
            top_and_bottom(ShellSlideKind::MoviesAndCredits),
            (3, true, false)
        );
        assert_eq!(top_and_bottom(ShellSlideKind::MovieList), (1, true, false));
        // Retail `0x102` slides Start Game and Choose Map; VERA's Game Options
        // slides between them and its Setups under them.
        assert_eq!(top_and_bottom(ShellSlideKind::Skirmish), (4, true, true));
        // Retail `0xD5` slides Keyboard and Network; VERA leaves Network out
        // and slides its six page buttons under Keyboard, then its Mods
        // button where the build has mods.
        assert_eq!(
            crate::ui::shell::slide::slide_spec_for(DialogResourceId(0x00D5))
                .unwrap()
                .top_buttons,
            2
        );
        let mods = u32::from(crate::ui::main_menu_dialogs::options::HAS_MODS);
        assert_eq!(
            top_and_bottom(ShellSlideKind::Options),
            (7 + mods, true, false)
        );
    }

    #[test]
    fn modern_wol_slides_only_animate_the_visible_button_faces() {
        let lobby = crate::ui::wol_lobby::WolLobbyState::new(false, true);
        let column = ShellSlideKind::WolLobby.column(9, Some(lobby.page_spec()));
        assert_eq!(
            (column.top_buttons, column.bottom_button, column.top_panel),
            (6, true, false)
        );
        assert_eq!(ShellSlideKind::WolWelcome.column(9, None).top_buttons, 6);
        let retail = ShellSlideKind::WolLobby.column(9, None);
        assert_eq!((retail.top_buttons, retail.top_panel), (8, true));
    }
}
