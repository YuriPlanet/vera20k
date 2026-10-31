//! Explicit one-shot capture mode for exact shell certification.
//!
//! The capture session is app-level diagnostic state. It never enters the
//! deterministic simulation, never synthesizes OS input, and never rebuilds a
//! shell in a second renderer. It waits for the production main-menu dispatcher
//! to reach one ordinary steady frame, then records the final swapchain bytes.

use crate::util::time::{Duration, Instant};
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;

use crate::app::AppState;
use crate::app::frontend::main_menu_shell_render::Ra2tsDialogOwner;
use crate::app::frontend::shell_transition::{MainMenuEntryPresentToken, ShellSlideKind};
use crate::render::frame_readback::PendingBgra8Readback;
use crate::ui::game_screen::GameScreen;
use crate::ui::main_menu_dialogs::options::OptionsPage;
use crate::ui::main_menu_shell::MainMenuMovieBase;
use crate::ui::shell::static_reveal::Kind1PaintWindow;

pub(crate) mod fault_files;
pub(crate) mod loading_host_loss;
pub(crate) mod loading_runtime;
mod movies;
mod score;
mod skirmish;
pub(crate) mod startup_host_loss;
pub(crate) use skirmish::PresentedShell;

pub(crate) const CAPTURE_FLAG: &str = "--shell-capture";
const CHECKPOINT_MAIN_MENU_0XE2_STEADY: &str = "main-menu-0xe2-steady";
const CHECKPOINT_MAIN_MENU_0XE2_ENTRY_SEQUENCE: &str = "main-menu-0xe2-entry-sequence";
const CHECKPOINT_SKIRMISH_0X102_STEADY: &str = "skirmish-0x102-steady";
const CHECKPOINT_MOVIES_0X101_STEADY: &str = "movies-0x101-steady";
const CHECKPOINT_MAIN_MENU_0XE2_EXIT_CONFIRM: &str = "main-menu-0xe2-exit-confirm";
const CHECKPOINT_MOVIE_LIST_0X129_STEADY: &str = "movie-list-0x129-steady";
const CHECKPOINT_MOVIE_LIST_0X129_SELECTED: &str = "movie-list-0x129-selected";
const CHECKPOINT_MOVIE_LIST_0X129_FULL: &str = "movie-list-0x129-full";
const CHECKPOINT_MOVIE_LIST_0X129_FULL_DOWN2: &str = "movie-list-0x129-full-down2";
const CHECKPOINT_MOVIE_LIST_0X129_BACK_FIRST_FRAME: &str = "movie-list-0x129-back-first-frame";
const CHECKPOINT_CAMPAIGN_0X94_STEADY: &str = "campaign-0x94-steady";
const CHECKPOINT_CAMPAIGN_0X94_SLIDER_LEFT: &str = "campaign-0x94-slider-left";
const CHECKPOINT_CAMPAIGN_0X94_SLIDER_RIGHT: &str = "campaign-0x94-slider-right";
/// The difficulty slider pressed at its left end and held with the pointer
/// over Back: the slider keeps the mouse, so the status line keeps its help
/// and Back stays unlit.
const CHECKPOINT_CAMPAIGN_0X94_SLIDER_HELD: &str = "campaign-0x94-slider-held";
const CHECKPOINT_CAMPAIGN_0X94_ENTRY_PREFIX: &str = "campaign-0x94-entry-tick-";
const CHECKPOINT_CAMPAIGN_ALL1_LOADING_FIRST: &str = "campaign-all1-loading-first-frame";
const CHECKPOINT_CAMPAIGN_SOV1_LOADING_FIRST: &str = "campaign-sov1-loading-first-frame";
const CHECKPOINT_CAMPAIGN_ALL1_FIRST_LIVE: &str = "campaign-all1-first-live-frame";
const CHECKPOINT_CAMPAIGN_SOV1_FIRST_LIVE: &str = "campaign-sov1-first-live-frame";
const CHECKPOINT_CAMPAIGN_ALL1_ABORT_RETURN: &str = "campaign-all1-abort-return";
const CHECKPOINT_CAMPAIGN_SOV1_ABORT_RETURN: &str = "campaign-sov1-abort-return";
const CHECKPOINT_CAMPAIGN_ALL1_ABORT_RESTART_SHOWN: &str = "campaign-all1-abort-restart-shown";
const CHECKPOINT_CAMPAIGN_ALL1_ABORT_RESTART: &str = "campaign-all1-abort-restart";
const CHECKPOINT_CAMPAIGN_ALL1_DEFEAT_REPLAY_PROMPT: &str = "campaign-all1-defeat-replay-prompt";
const CHECKPOINT_CAMPAIGN_ALL1_DEFEAT_REPLAY_YES: &str = "campaign-all1-defeat-replay-yes";
const CHECKPOINT_CAMPAIGN_ALL1_DEFEAT_REPLAY_NO: &str = "campaign-all1-defeat-replay-no";
const CHECKPOINT_CAMPAIGN_ALL1_SAVED_VICTORY: &str = "campaign-all1-saved-victory";
const CHECKPOINT_CAMPAIGN_ALL1_SAVED_RESTART: &str = "campaign-all1-saved-restart";
const CHECKPOINT_CAMPAIGN_ALL1_MENU_REPLAY: &str = "campaign-all1-menu-replay";
const CHECKPOINT_CAMPAIGN_ALL1_RESTATE: &str = "campaign-all1-restate";
const CHECKPOINT_CAMPAIGN_SOV1_OPENING_LOCKED: &str = "campaign-sov1-opening-locked";
const CHECKPOINT_CAMPAIGN_SOV1_OPENING_PAN: &str = "campaign-sov1-opening-pan";
const CHECKPOINT_CAMPAIGN_SOV1_OPENING_FLASH: &str = "campaign-sov1-opening-flash";
const CHECKPOINT_CAMPAIGN_SOV1_OPENING_ARROWS: &str = "campaign-sov1-opening-arrows";
const CHECKPOINT_CAMPAIGN_SOV1_OPENING_UNLOCKED: &str = "campaign-sov1-opening-unlocked";
const CHECKPOINT_CAMPAIGN_SOV1_OPENING_RESTORE_UNLOCKED: &str =
    "campaign-sov1-opening-restore-unlocked";
const CHECKPOINT_CAMPAIGN_ALL1_OPENING_LOAD_UNLOCKED: &str = "campaign-all1-opening-load-unlocked";
const CHECKPOINT_CAMPAIGN_ALL1_OPENING_TICK_PREFIX: &str = "campaign-all1-opening-tick-";
const CHECKPOINT_CAMPAIGN_SOV1_OPENING_TICK_PREFIX: &str = "campaign-sov1-opening-tick-";
const CHECKPOINT_CAMPAIGN_BACK_RETURN: &str = "campaign-back-return";
const CHECKPOINT_LOAD_SAVED_GAME_0XB7_STEADY: &str = "load-saved-game-0xb7-steady";
const CHECKPOINT_LOAD_SAVED_GAME_0XB7_ENTRY_PREFIX: &str = "load-saved-game-0xb7-entry-tick-";
const CHECKPOINT_OPTIONS_0XD5_STEADY: &str = "options-0xd5-steady";
/// Vera Options pressed on the settled `0xD5`: VERA's page settled.
const CHECKPOINT_OPTIONS_VERA_STEADY: &str = "options-vera-steady";
const NETWORK_OPTIONS_CHECKPOINTS: [(&str, movies::NetworkOptionsTarget); 7] = [
    (
        "network-options-0xd7-steady",
        movies::NetworkOptionsTarget::Steady,
    ),
    (
        "network-options-0xd7-cards",
        movies::NetworkOptionsTarget::Cards,
    ),
    (
        "network-options-0xd7-invalid-port",
        movies::NetworkOptionsTarget::InvalidPort,
    ),
    (
        "network-options-0xd7-warning-1",
        movies::NetworkOptionsTarget::Warning1,
    ),
    (
        "network-options-0xd7-warning-2",
        movies::NetworkOptionsTarget::Warning2,
    ),
    (
        "network-options-0xd7-back",
        movies::NetworkOptionsTarget::Back,
    ),
    (
        "network-options-0xd7-save-return",
        movies::NetworkOptionsTarget::SaveReturn,
    ),
];
const CHECKPOINT_MAIN_MENU_0XE2_NETWORK_BOUNCE: &str = "main-menu-0xe2-network-bounce";
const CHECKPOINT_WOL_0X10E_STEADY: &str = "wol-0x10e-steady";
const CHECKPOINT_WOL_0X10E_ENTRY_PREFIX: &str = "wol-0x10e-entry-tick-";
const CHECKPOINT_WOL_0X10E_HOVER_QUICK_MATCH: &str = "wol-0x10e-hover-quick-match";
const CHECKPOINT_WOL_0X10E_API_MISSING: &str = "wol-0x10e-api-missing";
const CHECKPOINT_WOL_0X10E_BACK: &str = "wol-0x10e-back";
/// Quick Match's centre at 800x600.
const WOL_QUICK_MATCH_POINT: (i32, i32) = (720, 220);
const CHECKPOINT_OPTIONS_0XD5_ENTRY_PREFIX: &str = "options-0xd5-entry-tick-";
/// `options-0xd5-hover-<control>`: the pointer rests on a control (like the
/// retail helper's hover capture) so the status line shows its help.
const OPTIONS_0XD5_HOVER_CHECKPOINTS: [(&str, (i32, i32)); 3] = [
    ("options-0xd5-hover-difficulty", (180, 214)),
    ("options-0xd5-hover-tooltips", (175, 315)),
    ("options-0xd5-hover-music", (140, 480)),
];
/// `skirmish-0x102-hover-<control>`: once `0x102` settles the pointer rests
/// on a control, as in the retail stills `sk-hover-start.png` and
/// `sk-hover-short.png`.
const SKIRMISH_0X102_HOVER_CHECKPOINTS: [(&str, (i32, i32)); 2] = [
    ("skirmish-0x102-hover-start", (720, 262)),
    ("skirmish-0x102-hover-short-game", (110, 294)),
];
/// Slider press points of the retail comparison stills.
const CAMPAIGN_SLIDER_LEFT_POINT: (i32, i32) = (190, 275);
const CAMPAIGN_SLIDER_RIGHT_POINT: (i32, i32) = (440, 275);
/// Back's centre at 800x600.
const CAMPAIGN_BACK_POINT: (i32, i32) = (722, 556);
const CHECKPOINT_CREDITS_ROLL_FRAME_PREFIX: &str = "credits-roll-frame-";
const CHECKPOINT_SNEAK_PEEK_FRAME_PREFIX: &str = "sneak-peek-frame-";
const CHECKPOINT_MAIN_MENU_0XE2_SLIDE_OUT_PREFIX: &str = "main-menu-0xe2-slide-out-tick-";
const CHECKPOINT_MOVIE_LIST_0X129_SLIDE_OUT_PREFIX: &str = "movie-list-0x129-slide-out-tick-";
const CHECKPOINT_SKIRMISH_0X102_BACK_SLIDE_OUT_PREFIX: &str = "skirmish-0x102-back-slide-out-tick-";
const CHECKPOINT_SKIRMISH_0X102_ENTRY_PREFIX: &str = "skirmish-0x102-entry-tick-";
const CHECKPOINT_SKIRMISH_0X6B_STEADY: &str = "skirmish-0x6b-steady";
const CHECKPOINT_SKIRMISH_0X6B_ENTRY_PREFIX: &str = "skirmish-0x6b-entry-tick-";
const CHECKPOINT_SKIRMISH_0X105_STEADY: &str = "skirmish-0x105-steady";
const CHECKPOINT_SKIRMISH_0X105_ENTRY_PREFIX: &str = "skirmish-0x105-entry-tick-";
/// Cancel on `0x105`, then `0x6B`'s entry slide held at one tick.
const CHECKPOINT_SKIRMISH_0X105_CANCEL_PREFIX: &str = "skirmish-0x105-cancel-tick-";
const CHECKPOINT_SKIRMISH_0X105_CANCEL_STEADY: &str = "skirmish-0x105-cancel-steady";
const CHECKPOINT_SKIRMISH_0X105_USE_MAP: &str = "skirmish-0x105-use-map";
const CHECKPOINT_SKIRMISH_0X105_SEED_BROWSER_RETURN: &str = "skirmish-0x105-seed-browser-return";
/// Terrain Type's list open with the pointer below it on Map Size's face; the
/// status line keeps Terrain Type's help (`ej-list-off.png`).
const CHECKPOINT_SKIRMISH_0X105_LIST_OPEN: &str = "skirmish-0x105-list-open";
/// Use Map with an AI row the random map cannot hold, then the eject box's
/// Cancel (`0x6B` settled again) or OK (`0x102` settled with the map).
const CHECKPOINT_SKIRMISH_0X105_EJECT_CANCEL: &str = "skirmish-0x105-eject-cancel";
const CHECKPOINT_SKIRMISH_0X105_EJECT_OK: &str = "skirmish-0x105-eject-ok";
/// The same route's eject box over no dialog (`ej-box.png`).
const CHECKPOINT_SKIRMISH_0X105_EJECT_BOX: &str = "skirmish-0x105-eject-box";
/// `skirmish-0x105-hover-<control>`: `0x105` settled with the pointer
/// resting on a control, as in `rmg-hover-load.png` and
/// `rmg-hover-surprise.png`.
const SKIRMISH_0X105_HOVER_CHECKPOINTS: [(&str, (i32, i32)); 2] = [
    ("skirmish-0x105-hover-load", (720, 262)),
    ("skirmish-0x105-hover-surprise", (170, 430)),
];
const CHECKPOINT_SKIRMISH_0X102_CHOOSE_MAP_RETURN: &str = "skirmish-0x102-choose-map-return";
const CHECKPOINT_SKIRMISH_START_BLANK: &str = "skirmish-start-blank";
const CHECKPOINT_SKIRMISH_LOADING_FIRST_FRAME: &str = "skirmish-loading-first-frame";
const CHECKPOINT_SKIRMISH_0X6B_EJECT_BOX: &str = "skirmish-0x6b-eject-box";
const CHECKPOINT_SKIRMISH_AFTER_QUIT: &str = "skirmish-after-quit";
const CHECKPOINT_SKIRMISH_DEFEAT_SCORE: &str = "skirmish-defeat-score";
const CHECKPOINT_SKIRMISH_DEFEAT_CONTINUE: &str = "skirmish-defeat-continue";
const CHECKPOINT_SKIRMISH_DIPLOMACY: &str = "skirmish-diplomacy";
const CHECKPOINT_SCORE_0X108_STEADY: &str = "score-0x108-steady";
const CHECKPOINT_CAMPAIGN_SCORE_0X103_STEADY: &str = "campaign-score-0x103-steady";
const CHECKPOINT_CAMPAIGN_SCORE_0X103_ANIMATION_SEQUENCE: &str =
    "campaign-score-0x103-animation-sequence";
const CHECKPOINT_SCORE_0X108_ENTRY_PREFIX: &str = "score-0x108-entry-tick-";
const CHECKPOINT_SCORE_0X108_HOVER_CONTINUE: &str = "score-0x108-hover-continue";
const CHECKPOINT_SCORE_0X108_LEAVE_CONTINUE: &str = "score-0x108-leave-continue";
const CHECKPOINT_SCORE_0X108_SLIDE_OUT_PREFIX: &str = "score-0x108-slide-out-tick-";
const EXPECTED_WIDTH: u32 = 800;
const EXPECTED_HEIGHT: u32 = 600;
const EXPECTED_CURSOR_X: u32 = 400;
const EXPECTED_CURSOR_Y: u32 = 300;
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(15);
const CAPTURE_FRAME_INTERVAL: Duration = Duration::from_millis(8);
const MAX_CAPTURE_FRAMES: u32 = 10_000;
const CAPTURE_SCHEMA: &str = "vera20k.shell-capture.v2";
const ENTRY_SEQUENCE_SCHEMA: &str = "vera20k.shell-entry-sequence-capture.v1";
const FRAME_FILE_NAME: &str = "frame.bgra";
const ENTRY_SEQUENCE_FRAMES_FILE_NAME: &str = "frames.bgra";
const MANIFEST_FILE_NAME: &str = "capture.json";
const FRAME_BYTE_LENGTH: u64 = EXPECTED_WIDTH as u64 * EXPECTED_HEIGHT as u64 * 4;
/// Ticks of the `0xE2` entry slide at 800x600: 9 tile rows + 9 (`0x006071E0`).
const ENTRY_SEQUENCE_FRAMES: u8 = 18;
const ENTRY_SEQUENCE_TERMINAL_TICK: u8 = ENTRY_SEQUENCE_FRAMES - 1;
const ENTRY_SEQUENCE_BYTE_LENGTH: u64 = FRAME_BYTE_LENGTH * ENTRY_SEQUENCE_FRAMES as u64;

#[derive(Debug)]
pub enum AppLaunchMode {
    Interactive,
    ShellCapture(ShellCaptureRequest),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellCaptureCheckpoint {
    MainMenu0xE2Steady,
    MainMenu0xE2EntrySequence,
    Skirmish0x102Steady,
    MoviesPage0x101Steady,
    /// Exit Game on `0xE2`: after the teardown slide, the confirmation over
    /// the empty shell backdrop (state 6).
    MainMenu0xE2ExitConfirm,
    MovieList0x129Steady,
    MovieList0x129Selected,
    /// All 17 movies unlocked (a scrollbar is shown), optionally after two
    /// down-arrow presses.
    MovieList0x129Full,
    MovieList0x129FullDown2,
    /// Back on the movie list: the first frame after its teardown slide,
    /// which must already be the recreated `0x101`'s entry slide at tick 0.
    MovieList0x129BackFirstFrame,
    /// Single Player -> New Campaign: campaign selection `0x94` settled.
    Campaign0x94Steady,
    /// The same after a press on the difficulty slider's left or right end.
    Campaign0x94SliderLeft,
    Campaign0x94SliderRight,
    /// A press on the slider held with the pointer over Back.
    Campaign0x94SliderHeld,
    /// Its entry slide held at one tick (`campaign-0x94-entry-tick-<N>`).
    Campaign0x94Entry(u32),
    /// Real emblem press/release through the menu route, then the first
    /// native loading frame. Intro/Brief is skipped by ordinary Escape release.
    CampaignLoadingFirst(crate::ui::campaign_shell::CampaignSide),
    /// The same route's first rendered InGame frame, with simulation frozen
    /// at tick zero. Authored opening triggers (#1308) are not certified.
    CampaignFirstLive(crate::ui::campaign_shell::CampaignSide),
    CampaignInitialSaved(crate::ui::campaign_shell::CampaignSide),
    CampaignExplicitFirstLive(&'static str),
    /// First installed campaign, then physical Escape/B5 Abort/B6 Leave and
    /// ordinary queued EXIT/audio teardown, read back on the settled main menu.
    CampaignAbortReturn(crate::ui::campaign_shell::CampaignSide),
    /// First installed ALL01, then B6 with Restart, B6 Restart into the
    /// restarted mission, or a forced loss to its `TXT_TO_REPLAY` box (and Yes).
    CampaignRestart(movies::RestartTarget),
    /// An authored campaign opening (SOV01, or ALL01 for the in-lock load),
    /// through the normal loader and exact production sim/app steps. These
    /// receipts are integration observations.
    CampaignOpening(movies::OpeningTarget),
    /// Ordinary Back from campaign selection to the recreated Single Player.
    CampaignBackReturn,
    /// Single Player -> Load Saved Game: `0xB7` settled. Needs a readable
    /// save in the saves directory.
    LoadSavedGame0xB7Steady,
    /// Its entry slide held at one tick
    /// (`load-saved-game-0xb7-entry-tick-<N>`).
    LoadSavedGame0xB7Entry(u32),
    /// Main Menu -> Options: `0xD5` settled.
    Options0xD5Steady,
    /// Then Vera Options: VERA's page settled.
    OptionsVeraSteady,
    /// Ordinary D5 -> D7 with explicit service/settings inputs.
    NetworkOptions(movies::NetworkOptionsTarget),
    Lan(movies::LanTarget),
    Wol(movies::WolTarget),
    /// Network on `0xE2`: after its teardown slide and a new `0xE2`'s entry
    /// slide, the settled main menu.
    MainMenu0xE2NetworkBounce,
    /// Main Menu -> Internet: `0x10E` settled.
    Wol0x10ESteady,
    /// Its entry slide held at one tick (`wol-0x10e-entry-tick-<N>`).
    Wol0x10EEntry(u32),
    /// Settled with the pointer on Quick Match.
    Wol0x10EHoverQuickMatch,
    /// My Information pressed: the `TXT_APIMISSING` box `0xD0`.
    Wol0x10EApiMissing,
    /// Main Menu pressed: the new `0xE2` settled.
    Wol0x10EBack,
    /// Its entry slide held at one tick (`options-0xd5-entry-tick-<N>`).
    Options0xD5Entry(u32),
    /// Settled with the pointer resting on a control
    /// (`OPTIONS_0XD5_HOVER_CHECKPOINTS` index).
    Options0xD5Hover(usize),
    /// Show_Credits pinned at one roll frame (`credits-roll-frame-<N>`).
    CreditsRollFrame(u64),
    /// Sneak Peeks Play_Movie pinned at one video frame (`sneak-peek-frame-<N>`).
    SneakPeekFrame(usize),
    /// Exit Game on `0xE2`, its teardown slide held at one tick
    /// (`main-menu-0xe2-slide-out-tick-<N>`).
    MainMenu0xE2SlideOut(u32),
    /// Back on the movie list, its teardown slide held at one tick
    /// (`movie-list-0x129-slide-out-tick-<N>`).
    MovieList0x129SlideOut(u32),
    /// Back on Skirmish, its teardown slide held at one tick
    /// (`skirmish-0x102-back-slide-out-tick-<N>`).
    Skirmish0x102BackSlideOut(u32),
    /// Skirmish opened from Single Player, its entry slide held at one tick
    /// (`skirmish-0x102-entry-tick-<N>`).
    Skirmish0x102Entry(u32),
    /// Skirmish settled with the pointer resting on a control
    /// (`SKIRMISH_0X102_HOVER_CHECKPOINTS` index).
    Skirmish0x102Hover(usize),
    /// Choose Map's Create Random Map: `0x105` settled.
    Skirmish0x105Steady,
    /// `0x105`'s entry slide held at one tick.
    Skirmish0x105Entry(u32),
    /// `0x105` settled with the pointer resting on a control
    /// (`SKIRMISH_0X105_HOVER_CHECKPOINTS` index).
    Skirmish0x105Hover(usize),
    /// Cancel on `0x105`: `0x6B`'s entry slide held at one tick.
    Skirmish0x105CancelEntry(u32),
    /// Cancel on `0x105`: `0x6B` settled again.
    Skirmish0x105CancelSteady,
    /// Use Map on `0x105`: the map generates and `0x102` settles with it.
    Skirmish0x105UseMap,
    /// Save Map on `0x105` opens the seed browser over it; the browser's
    /// Back uncovers `0x105`, which settles without an entry slide.
    Skirmish0x105SeedBrowserReturn,
    /// Terrain Type's list open on `0x105`, the pointer resting on a row.
    Skirmish0x105ListOpen,
    /// Use Map on `0x105` asks to eject AI players; the answer's dialog
    /// settles (`CHECKPOINT_SKIRMISH_0X105_EJECT_*`).
    Skirmish0x105Eject(crate::ui::skirmish_shell::EjectPromptButton),
    /// The same route's eject box, unanswered.
    Skirmish0x105EjectBox,
    /// Choose Map `0x6B` settled after Skirmish's Choose Map.
    Skirmish0x6BSteady,
    /// Choose Map `0x6B`'s entry slide held at one tick
    /// (`skirmish-0x6b-entry-tick-<N>`).
    Skirmish0x6BEntry(u32),
    /// Cancel on `0x6B`, then `0x102` settled after its new entry slide.
    Skirmish0x102ChooseMapReturn,
    /// Start Game on `0x102`: the black frame before the loading screen.
    SkirmishStartBlank,
    /// Start Game on `0x102`: the loading screen's first frame. The map load
    /// then runs to the end before the process exits.
    SkirmishLoadingFirstFrame,
    /// Use Map on `0x6B`'s first map with AI rows that do not fit: the eject
    /// box over the empty backdrop.
    Skirmish0x6BEjectBox,
    /// Start Game, a short game, Leave through the in-game abort: the new
    /// `0x102` the shell resumes on, settled.
    SkirmishAfterQuit,
    /// Start Game, deploy the MCV and sell the Construction Yard (Short
    /// Game): the score page after the real defeat, settled.
    SkirmishDefeatScore,
    /// The same, then Continue: the new `0x102`, settled.
    SkirmishDefeatContinue,
    SkirmishDiplomacy,
    /// The score dialog `0x108` with the retail comparison game's values,
    /// settled.
    Score0x108Steady,
    /// Its entry slide held at one tick (`score-0x108-entry-tick-<N>`).
    Score0x108Entry(u32),
    /// Settled with the pointer on Continue.
    Score0x108HoverContinue,
    /// The pointer moved from Continue onto the bare art.
    Score0x108LeaveContinue,
    /// Continue pressed, its teardown slide held at one tick
    /// (`score-0x108-slide-out-tick-<N>`).
    Score0x108SlideOut(u32),
    /// Synthetic, settled rendering of the production campaign victory page
    /// `0x103`; fixture strings do not establish native parity.
    CampaignScore0x103Steady,
    /// Synthetic production-renderer sequence sampling visible `0x103` art
    /// frames, then exercising Continue close/reopen lifecycle. This is not
    /// native execution or parity evidence.
    CampaignScore0x103AnimationSequence,
}

impl ShellCaptureCheckpoint {
    fn parse(value: &str) -> Result<Self> {
        if let Some((_, target)) = movies::LAN_CHECKPOINTS
            .iter()
            .find(|(name, _)| *name == value)
        {
            return Ok(Self::Lan(*target));
        }
        if let Some((_, target)) = movies::WOL_CHECKPOINTS
            .iter()
            .find(|(name, _)| *name == value)
        {
            return Ok(Self::Wol(*target));
        }
        if let Some(tick) = value.strip_prefix(CHECKPOINT_CAMPAIGN_ALL1_OPENING_TICK_PREFIX) {
            let tick: u64 = tick
                .parse()
                .context("campaign opening tick is not an integer")?;
            ensure!(
                (1..=2500).contains(&tick),
                "campaign opening tick must be 1..=2500"
            );
            return Ok(Self::CampaignOpening(movies::OpeningTarget::AlliedTick(
                tick,
            )));
        }
        if let Some(tick) = value.strip_prefix(CHECKPOINT_CAMPAIGN_SOV1_OPENING_TICK_PREFIX) {
            let tick: u64 = tick
                .parse()
                .context("campaign encounter tick is not an integer")?;
            ensure!(
                (1..=10000).contains(&tick),
                "campaign encounter tick must be 1..=10000"
            );
            return Ok(Self::CampaignOpening(movies::OpeningTarget::SovietTick(
                tick,
            )));
        }
        if let Some(tick) = value.strip_prefix("campaign-sov1-navy-tick-") {
            let tick: u64 = tick
                .parse()
                .context("campaign encounter tick is not an integer")?;
            ensure!(
                (1100..=10000).contains(&tick),
                "campaign encounter tick must be 1100..=10000"
            );
            return Ok(Self::CampaignOpening(movies::OpeningTarget::SovietNavy(
                tick,
            )));
        }
        if let Some(tick) = value.strip_prefix("campaign-sov07-groups-tick-") {
            let tick: u64 = tick
                .parse()
                .context("campaign group tick is not an integer")?;
            ensure!(
                (301..=10000).contains(&tick),
                "campaign group tick must be 301..=10000"
            );
            return Ok(Self::CampaignOpening(movies::OpeningTarget::SovietGroups(
                tick,
            )));
        }
        if let Some(tick) = value.strip_prefix("campaign-sov02-fortresses-tick-") {
            let tick: u64 = tick
                .parse()
                .context("campaign fortress tick is not an integer")?;
            ensure!(
                (301..=10000).contains(&tick),
                "campaign fortress tick must be 301..=10000"
            );
            return Ok(Self::CampaignOpening(
                movies::OpeningTarget::SovietFortresses(tick),
            ));
        }
        if let Some(frame) = value.strip_prefix(CHECKPOINT_CREDITS_ROLL_FRAME_PREFIX) {
            let frame: u64 = frame
                .parse()
                .with_context(|| format!("credits roll frame is not an integer: {frame:?}"))?;
            ensure!(frame > 0, "credits roll frames start at 1");
            return Ok(Self::CreditsRollFrame(frame));
        }
        if let Some(frame) = value.strip_prefix(CHECKPOINT_SNEAK_PEEK_FRAME_PREFIX) {
            let frame: usize = frame
                .parse()
                .with_context(|| format!("movie frame is not an integer: {frame:?}"))?;
            return Ok(Self::SneakPeekFrame(frame));
        }
        for (prefix, kind, checkpoint) in [
            (
                CHECKPOINT_MAIN_MENU_0XE2_SLIDE_OUT_PREFIX,
                ShellSlideKind::MainMenu,
                Self::MainMenu0xE2SlideOut as fn(u32) -> Self,
            ),
            (
                CHECKPOINT_MOVIE_LIST_0X129_SLIDE_OUT_PREFIX,
                ShellSlideKind::MovieList,
                Self::MovieList0x129SlideOut,
            ),
            (
                CHECKPOINT_SKIRMISH_0X102_BACK_SLIDE_OUT_PREFIX,
                ShellSlideKind::Skirmish,
                Self::Skirmish0x102BackSlideOut,
            ),
            (
                CHECKPOINT_SKIRMISH_0X102_ENTRY_PREFIX,
                ShellSlideKind::Skirmish,
                Self::Skirmish0x102Entry,
            ),
            (
                CHECKPOINT_SKIRMISH_0X105_ENTRY_PREFIX,
                ShellSlideKind::RandomMap,
                Self::Skirmish0x105Entry,
            ),
            (
                CHECKPOINT_SKIRMISH_0X105_CANCEL_PREFIX,
                ShellSlideKind::ChooseMap,
                Self::Skirmish0x105CancelEntry,
            ),
            (
                CHECKPOINT_SKIRMISH_0X6B_ENTRY_PREFIX,
                ShellSlideKind::ChooseMap,
                Self::Skirmish0x6BEntry,
            ),
            (
                CHECKPOINT_CAMPAIGN_0X94_ENTRY_PREFIX,
                ShellSlideKind::Campaign,
                Self::Campaign0x94Entry,
            ),
            (
                CHECKPOINT_LOAD_SAVED_GAME_0XB7_ENTRY_PREFIX,
                ShellSlideKind::LoadSavedGame,
                Self::LoadSavedGame0xB7Entry,
            ),
            (
                CHECKPOINT_OPTIONS_0XD5_ENTRY_PREFIX,
                ShellSlideKind::Options,
                Self::Options0xD5Entry,
            ),
            (
                CHECKPOINT_WOL_0X10E_ENTRY_PREFIX,
                ShellSlideKind::WolWelcome,
                Self::Wol0x10EEntry,
            ),
            (
                CHECKPOINT_SCORE_0X108_ENTRY_PREFIX,
                ShellSlideKind::Score,
                Self::Score0x108Entry,
            ),
            (
                CHECKPOINT_SCORE_0X108_SLIDE_OUT_PREFIX,
                ShellSlideKind::Score,
                Self::Score0x108SlideOut,
            ),
        ] {
            let Some(tick) = value.strip_prefix(prefix) else {
                continue;
            };
            let tick: u32 = tick
                .parse()
                .with_context(|| format!("slide tick is not an integer: {tick:?}"))?;
            // The slide shows ticks 0 up to its loop bound, exclusive: the
            // panel's tile rows + 9 at the capture's 800x600.
            let rows = crate::ui::shell::geom::right_panel_rects(
                EXPECTED_WIDTH as i32,
                EXPECTED_HEIGHT as i32,
            )
            .tile_count as u32;
            let last = kind.column(rows, None).total_ticks() - 1;
            ensure!(tick <= last, "{value}: the slide shows ticks 0..={last}");
            return Ok(checkpoint(tick));
        }
        match value {
            CHECKPOINT_MAIN_MENU_0XE2_STEADY => Ok(Self::MainMenu0xE2Steady),
            CHECKPOINT_MAIN_MENU_0XE2_ENTRY_SEQUENCE => Ok(Self::MainMenu0xE2EntrySequence),
            CHECKPOINT_SKIRMISH_0X102_STEADY => Ok(Self::Skirmish0x102Steady),
            CHECKPOINT_SKIRMISH_0X6B_STEADY => Ok(Self::Skirmish0x6BSteady),
            CHECKPOINT_SKIRMISH_0X105_STEADY => Ok(Self::Skirmish0x105Steady),
            CHECKPOINT_SKIRMISH_0X105_CANCEL_STEADY => Ok(Self::Skirmish0x105CancelSteady),
            CHECKPOINT_SKIRMISH_0X105_USE_MAP => Ok(Self::Skirmish0x105UseMap),
            CHECKPOINT_SKIRMISH_0X105_LIST_OPEN => Ok(Self::Skirmish0x105ListOpen),
            CHECKPOINT_SKIRMISH_0X105_EJECT_BOX => Ok(Self::Skirmish0x105EjectBox),
            CHECKPOINT_SKIRMISH_0X105_SEED_BROWSER_RETURN => {
                Ok(Self::Skirmish0x105SeedBrowserReturn)
            }
            CHECKPOINT_SKIRMISH_0X105_EJECT_CANCEL => Ok(Self::Skirmish0x105Eject(
                crate::ui::skirmish_shell::EjectPromptButton::Cancel,
            )),
            CHECKPOINT_SKIRMISH_0X105_EJECT_OK => Ok(Self::Skirmish0x105Eject(
                crate::ui::skirmish_shell::EjectPromptButton::Ok,
            )),
            CHECKPOINT_SKIRMISH_0X102_CHOOSE_MAP_RETURN => Ok(Self::Skirmish0x102ChooseMapReturn),
            CHECKPOINT_SKIRMISH_START_BLANK => Ok(Self::SkirmishStartBlank),
            CHECKPOINT_SKIRMISH_LOADING_FIRST_FRAME => Ok(Self::SkirmishLoadingFirstFrame),
            CHECKPOINT_SKIRMISH_0X6B_EJECT_BOX => Ok(Self::Skirmish0x6BEjectBox),
            CHECKPOINT_SKIRMISH_AFTER_QUIT => Ok(Self::SkirmishAfterQuit),
            CHECKPOINT_SKIRMISH_DEFEAT_SCORE => Ok(Self::SkirmishDefeatScore),
            CHECKPOINT_SKIRMISH_DEFEAT_CONTINUE => Ok(Self::SkirmishDefeatContinue),
            CHECKPOINT_SKIRMISH_DIPLOMACY => Ok(Self::SkirmishDiplomacy),
            CHECKPOINT_SCORE_0X108_STEADY => Ok(Self::Score0x108Steady),
            CHECKPOINT_CAMPAIGN_SCORE_0X103_STEADY => Ok(Self::CampaignScore0x103Steady),
            CHECKPOINT_CAMPAIGN_SCORE_0X103_ANIMATION_SEQUENCE => {
                Ok(Self::CampaignScore0x103AnimationSequence)
            }
            CHECKPOINT_SCORE_0X108_HOVER_CONTINUE => Ok(Self::Score0x108HoverContinue),
            CHECKPOINT_SCORE_0X108_LEAVE_CONTINUE => Ok(Self::Score0x108LeaveContinue),
            CHECKPOINT_MOVIES_0X101_STEADY => Ok(Self::MoviesPage0x101Steady),
            CHECKPOINT_MAIN_MENU_0XE2_EXIT_CONFIRM => Ok(Self::MainMenu0xE2ExitConfirm),
            CHECKPOINT_MOVIE_LIST_0X129_STEADY => Ok(Self::MovieList0x129Steady),
            CHECKPOINT_MOVIE_LIST_0X129_SELECTED => Ok(Self::MovieList0x129Selected),
            CHECKPOINT_MOVIE_LIST_0X129_FULL => Ok(Self::MovieList0x129Full),
            CHECKPOINT_MOVIE_LIST_0X129_FULL_DOWN2 => Ok(Self::MovieList0x129FullDown2),
            CHECKPOINT_MOVIE_LIST_0X129_BACK_FIRST_FRAME => Ok(Self::MovieList0x129BackFirstFrame),
            CHECKPOINT_CAMPAIGN_0X94_STEADY => Ok(Self::Campaign0x94Steady),
            CHECKPOINT_CAMPAIGN_0X94_SLIDER_LEFT => Ok(Self::Campaign0x94SliderLeft),
            CHECKPOINT_CAMPAIGN_0X94_SLIDER_RIGHT => Ok(Self::Campaign0x94SliderRight),
            CHECKPOINT_CAMPAIGN_0X94_SLIDER_HELD => Ok(Self::Campaign0x94SliderHeld),
            CHECKPOINT_CAMPAIGN_ALL1_LOADING_FIRST => Ok(Self::CampaignLoadingFirst(
                crate::ui::campaign_shell::CampaignSide::Allied,
            )),
            CHECKPOINT_CAMPAIGN_SOV1_LOADING_FIRST => Ok(Self::CampaignLoadingFirst(
                crate::ui::campaign_shell::CampaignSide::Soviet,
            )),
            CHECKPOINT_CAMPAIGN_ALL1_FIRST_LIVE => Ok(Self::CampaignFirstLive(
                crate::ui::campaign_shell::CampaignSide::Allied,
            )),
            CHECKPOINT_CAMPAIGN_SOV1_FIRST_LIVE => Ok(Self::CampaignFirstLive(
                crate::ui::campaign_shell::CampaignSide::Soviet,
            )),
            "campaign-sov02-objective-expiry" => Ok(Self::CampaignRestart(
                movies::RestartTarget::ObjectiveExpiry,
            )),
            "campaign-all1-defeat-replay-enter" => Ok(Self::CampaignRestart(
                movies::RestartTarget::DefeatReturnedEnter,
            )),
            "campaign-all1-defeat-replay-numpad-enter" => Ok(Self::CampaignRestart(
                movies::RestartTarget::DefeatReturnedNumpadEnter,
            )),
            "campaign-all1-defeat-replay-escape" => Ok(Self::CampaignRestart(
                movies::RestartTarget::DefeatReturnedEscape,
            )),
            "campaign-all1-defeat-replay-repeated-enter" => Ok(Self::CampaignRestart(
                movies::RestartTarget::DefeatReturnedRepeatedEnter,
            )),
            "campaign-sov05-first-live-frame" => {
                Ok(Self::CampaignExplicitFirstLive("SOV05UMD.MAP"))
            }
            "campaign-sov07-first-live-frame" => {
                Ok(Self::CampaignExplicitFirstLive("SOV07TMD.MAP"))
            }
            "campaign-all02-first-live-frame" => {
                Ok(Self::CampaignExplicitFirstLive("ALL02UMD.MAP"))
            }
            "campaign-all1-initial-save" => Ok(Self::CampaignInitialSaved(
                crate::ui::campaign_shell::CampaignSide::Allied,
            )),
            "campaign-sov1-initial-save" => Ok(Self::CampaignInitialSaved(
                crate::ui::campaign_shell::CampaignSide::Soviet,
            )),
            CHECKPOINT_CAMPAIGN_ALL1_ABORT_RETURN => Ok(Self::CampaignAbortReturn(
                crate::ui::campaign_shell::CampaignSide::Allied,
            )),
            CHECKPOINT_CAMPAIGN_SOV1_ABORT_RETURN => Ok(Self::CampaignAbortReturn(
                crate::ui::campaign_shell::CampaignSide::Soviet,
            )),
            CHECKPOINT_CAMPAIGN_ALL1_ABORT_RESTART_SHOWN => {
                Ok(Self::CampaignRestart(movies::RestartTarget::AbortShown))
            }
            CHECKPOINT_CAMPAIGN_ALL1_ABORT_RESTART => {
                Ok(Self::CampaignRestart(movies::RestartTarget::AbortRestarted))
            }
            CHECKPOINT_CAMPAIGN_ALL1_DEFEAT_REPLAY_PROMPT => {
                Ok(Self::CampaignRestart(movies::RestartTarget::DefeatPrompt))
            }
            CHECKPOINT_CAMPAIGN_ALL1_DEFEAT_REPLAY_NO => {
                Ok(Self::CampaignRestart(movies::RestartTarget::DefeatReturned))
            }
            CHECKPOINT_CAMPAIGN_ALL1_DEFEAT_REPLAY_YES => {
                Ok(Self::CampaignRestart(movies::RestartTarget::DefeatReplayed))
            }
            CHECKPOINT_CAMPAIGN_ALL1_SAVED_VICTORY => {
                Ok(Self::CampaignRestart(movies::RestartTarget::SavedVictory))
            }
            "campaign-sov6-alternate-home-victory" => Ok(Self::CampaignRestart(
                movies::RestartTarget::AlternateHomeVictory,
            )),
            "campaign-sov1-inherited-timer-victory" => Ok(Self::CampaignRestart(
                movies::RestartTarget::InheritedTimerVictory,
            )),
            "campaign-mapselect-failure-prompt" => Ok(Self::CampaignRestart(
                movies::RestartTarget::MapSelectFailurePrompt,
            )),
            "campaign-mapselect-failure-replayed" => Ok(Self::CampaignRestart(
                movies::RestartTarget::MapSelectFailureReplayed,
            )),
            "campaign-mapselect-failure-escape" => Ok(Self::CampaignRestart(
                movies::RestartTarget::MapSelectFailureEscape,
            )),
            CHECKPOINT_CAMPAIGN_ALL1_SAVED_RESTART => Ok(Self::CampaignRestart(
                movies::RestartTarget::SavedAbortRestarted,
            )),
            CHECKPOINT_CAMPAIGN_ALL1_MENU_REPLAY => Ok(Self::CampaignRestart(
                movies::RestartTarget::MenuDefeatReplayed,
            )),
            CHECKPOINT_CAMPAIGN_ALL1_RESTATE => {
                Ok(Self::CampaignRestart(movies::RestartTarget::Restate))
            }
            CHECKPOINT_CAMPAIGN_SOV1_OPENING_LOCKED => {
                Ok(Self::CampaignOpening(movies::OpeningTarget::Locked))
            }
            CHECKPOINT_CAMPAIGN_SOV1_OPENING_PAN => {
                Ok(Self::CampaignOpening(movies::OpeningTarget::CameraPan))
            }
            CHECKPOINT_CAMPAIGN_SOV1_OPENING_FLASH => {
                Ok(Self::CampaignOpening(movies::OpeningTarget::Flash))
            }
            CHECKPOINT_CAMPAIGN_SOV1_OPENING_ARROWS => {
                Ok(Self::CampaignOpening(movies::OpeningTarget::Arrows))
            }
            CHECKPOINT_CAMPAIGN_SOV1_OPENING_UNLOCKED => {
                Ok(Self::CampaignOpening(movies::OpeningTarget::Unlocked))
            }
            "campaign-sov1-opening-realtime-unlocked" => Ok(Self::CampaignOpening(
                movies::OpeningTarget::RealtimeUnlocked,
            )),
            "campaign-sov1-opening-timer-running" => {
                Ok(Self::CampaignOpening(movies::OpeningTarget::TimerRunning))
            }
            "campaign-sov1-opening-timer-expired" => {
                Ok(Self::CampaignOpening(movies::OpeningTarget::TimerExpired))
            }
            "campaign-sov2-radar-alert" => {
                Ok(Self::CampaignOpening(movies::OpeningTarget::RadarAlert))
            }
            "campaign-sov2-radar-restored" => {
                Ok(Self::CampaignOpening(movies::OpeningTarget::RadarRestored))
            }
            CHECKPOINT_CAMPAIGN_SOV1_OPENING_RESTORE_UNLOCKED => Ok(Self::CampaignOpening(
                movies::OpeningTarget::RestoreUnlocked,
            )),
            CHECKPOINT_CAMPAIGN_ALL1_OPENING_LOAD_UNLOCKED => Ok(Self::CampaignOpening(
                movies::OpeningTarget::LoadUnlockedWhileLocked,
            )),
            CHECKPOINT_CAMPAIGN_BACK_RETURN => Ok(Self::CampaignBackReturn),
            CHECKPOINT_LOAD_SAVED_GAME_0XB7_STEADY => Ok(Self::LoadSavedGame0xB7Steady),
            CHECKPOINT_OPTIONS_0XD5_STEADY => Ok(Self::Options0xD5Steady),
            CHECKPOINT_OPTIONS_VERA_STEADY => Ok(Self::OptionsVeraSteady),
            CHECKPOINT_MAIN_MENU_0XE2_NETWORK_BOUNCE => Ok(Self::MainMenu0xE2NetworkBounce),
            CHECKPOINT_WOL_0X10E_STEADY => Ok(Self::Wol0x10ESteady),
            CHECKPOINT_WOL_0X10E_HOVER_QUICK_MATCH => Ok(Self::Wol0x10EHoverQuickMatch),
            CHECKPOINT_WOL_0X10E_API_MISSING => Ok(Self::Wol0x10EApiMissing),
            CHECKPOINT_WOL_0X10E_BACK => Ok(Self::Wol0x10EBack),
            _ => {
                if let Some((_, target)) = NETWORK_OPTIONS_CHECKPOINTS
                    .iter()
                    .find(|(name, _)| *name == value)
                {
                    return Ok(Self::NetworkOptions(*target));
                }
                if let Some(index) = OPTIONS_0XD5_HOVER_CHECKPOINTS
                    .iter()
                    .position(|(name, _)| *name == value)
                {
                    return Ok(Self::Options0xD5Hover(index));
                }
                if let Some(index) = SKIRMISH_0X102_HOVER_CHECKPOINTS
                    .iter()
                    .position(|(name, _)| *name == value)
                {
                    return Ok(Self::Skirmish0x102Hover(index));
                }
                if let Some(index) = SKIRMISH_0X105_HOVER_CHECKPOINTS
                    .iter()
                    .position(|(name, _)| *name == value)
                {
                    return Ok(Self::Skirmish0x105Hover(index));
                }
                bail!("unsupported shell-capture checkpoint {value:?}")
            }
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::MainMenu0xE2Steady => CHECKPOINT_MAIN_MENU_0XE2_STEADY,
            Self::MainMenu0xE2EntrySequence => CHECKPOINT_MAIN_MENU_0XE2_ENTRY_SEQUENCE,
            Self::Skirmish0x102Steady => CHECKPOINT_SKIRMISH_0X102_STEADY,
            Self::MoviesPage0x101Steady => CHECKPOINT_MOVIES_0X101_STEADY,
            Self::MainMenu0xE2ExitConfirm => CHECKPOINT_MAIN_MENU_0XE2_EXIT_CONFIRM,
            Self::MovieList0x129Steady => CHECKPOINT_MOVIE_LIST_0X129_STEADY,
            Self::MovieList0x129Selected => CHECKPOINT_MOVIE_LIST_0X129_SELECTED,
            Self::MovieList0x129Full => CHECKPOINT_MOVIE_LIST_0X129_FULL,
            Self::MovieList0x129FullDown2 => CHECKPOINT_MOVIE_LIST_0X129_FULL_DOWN2,
            Self::MovieList0x129BackFirstFrame => CHECKPOINT_MOVIE_LIST_0X129_BACK_FIRST_FRAME,
            Self::Campaign0x94Steady => CHECKPOINT_CAMPAIGN_0X94_STEADY,
            Self::Campaign0x94SliderLeft => CHECKPOINT_CAMPAIGN_0X94_SLIDER_LEFT,
            Self::Campaign0x94SliderRight => CHECKPOINT_CAMPAIGN_0X94_SLIDER_RIGHT,
            Self::Campaign0x94SliderHeld => CHECKPOINT_CAMPAIGN_0X94_SLIDER_HELD,
            Self::Campaign0x94Entry(_) => "campaign-0x94-entry",
            Self::CampaignLoadingFirst(crate::ui::campaign_shell::CampaignSide::Allied) => {
                CHECKPOINT_CAMPAIGN_ALL1_LOADING_FIRST
            }
            Self::CampaignLoadingFirst(crate::ui::campaign_shell::CampaignSide::Soviet) => {
                CHECKPOINT_CAMPAIGN_SOV1_LOADING_FIRST
            }
            Self::CampaignFirstLive(crate::ui::campaign_shell::CampaignSide::Allied) => {
                CHECKPOINT_CAMPAIGN_ALL1_FIRST_LIVE
            }
            Self::CampaignFirstLive(crate::ui::campaign_shell::CampaignSide::Soviet) => {
                CHECKPOINT_CAMPAIGN_SOV1_FIRST_LIVE
            }
            Self::CampaignExplicitFirstLive("SOV05UMD.MAP") => "campaign-sov05-first-live-frame",
            Self::CampaignExplicitFirstLive("ALL02UMD.MAP") => "campaign-all02-first-live-frame",
            Self::CampaignExplicitFirstLive(_) => "campaign-sov07-first-live-frame",
            Self::CampaignInitialSaved(crate::ui::campaign_shell::CampaignSide::Allied) => {
                "campaign-all1-initial-save"
            }
            Self::CampaignInitialSaved(crate::ui::campaign_shell::CampaignSide::Soviet) => {
                "campaign-sov1-initial-save"
            }
            Self::CampaignAbortReturn(crate::ui::campaign_shell::CampaignSide::Allied) => {
                CHECKPOINT_CAMPAIGN_ALL1_ABORT_RETURN
            }
            Self::CampaignAbortReturn(crate::ui::campaign_shell::CampaignSide::Soviet) => {
                CHECKPOINT_CAMPAIGN_SOV1_ABORT_RETURN
            }
            Self::CampaignRestart(movies::RestartTarget::AbortShown) => {
                CHECKPOINT_CAMPAIGN_ALL1_ABORT_RESTART_SHOWN
            }
            Self::CampaignRestart(movies::RestartTarget::AbortRestarted) => {
                CHECKPOINT_CAMPAIGN_ALL1_ABORT_RESTART
            }
            Self::CampaignRestart(movies::RestartTarget::ObjectiveExpiry) => {
                "campaign-sov02-objective-expiry"
            }
            Self::CampaignRestart(movies::RestartTarget::DefeatPrompt) => {
                CHECKPOINT_CAMPAIGN_ALL1_DEFEAT_REPLAY_PROMPT
            }
            Self::CampaignRestart(movies::RestartTarget::DefeatReturned) => {
                CHECKPOINT_CAMPAIGN_ALL1_DEFEAT_REPLAY_NO
            }
            Self::CampaignRestart(movies::RestartTarget::DefeatReturnedEnter) => {
                "campaign-all1-defeat-replay-enter"
            }
            Self::CampaignRestart(movies::RestartTarget::DefeatReturnedNumpadEnter) => {
                "campaign-all1-defeat-replay-numpad-enter"
            }
            Self::CampaignRestart(movies::RestartTarget::DefeatReturnedEscape) => {
                "campaign-all1-defeat-replay-escape"
            }
            Self::CampaignRestart(movies::RestartTarget::DefeatReturnedRepeatedEnter) => {
                "campaign-all1-defeat-replay-repeated-enter"
            }
            Self::CampaignRestart(movies::RestartTarget::DefeatReplayed) => {
                CHECKPOINT_CAMPAIGN_ALL1_DEFEAT_REPLAY_YES
            }
            Self::CampaignRestart(movies::RestartTarget::SavedVictory) => {
                CHECKPOINT_CAMPAIGN_ALL1_SAVED_VICTORY
            }
            Self::CampaignRestart(movies::RestartTarget::AlternateHomeVictory) => {
                "campaign-sov6-alternate-home-victory"
            }
            Self::CampaignRestart(movies::RestartTarget::InheritedTimerVictory) => {
                "campaign-sov1-inherited-timer-victory"
            }
            Self::CampaignRestart(movies::RestartTarget::MapSelectFailurePrompt) => {
                "campaign-mapselect-failure-prompt"
            }
            Self::CampaignRestart(movies::RestartTarget::MapSelectFailureReplayed) => {
                "campaign-mapselect-failure-replayed"
            }
            Self::CampaignRestart(movies::RestartTarget::MapSelectFailureEscape) => {
                "campaign-mapselect-failure-escape"
            }
            Self::CampaignRestart(movies::RestartTarget::SavedAbortRestarted) => {
                CHECKPOINT_CAMPAIGN_ALL1_SAVED_RESTART
            }
            Self::CampaignRestart(movies::RestartTarget::MenuDefeatReplayed) => {
                CHECKPOINT_CAMPAIGN_ALL1_MENU_REPLAY
            }
            Self::CampaignRestart(movies::RestartTarget::Restate) => {
                CHECKPOINT_CAMPAIGN_ALL1_RESTATE
            }
            Self::CampaignOpening(movies::OpeningTarget::Locked) => {
                CHECKPOINT_CAMPAIGN_SOV1_OPENING_LOCKED
            }
            Self::CampaignOpening(movies::OpeningTarget::CameraPan) => {
                CHECKPOINT_CAMPAIGN_SOV1_OPENING_PAN
            }
            Self::CampaignOpening(movies::OpeningTarget::Flash) => {
                CHECKPOINT_CAMPAIGN_SOV1_OPENING_FLASH
            }
            Self::CampaignOpening(movies::OpeningTarget::Arrows) => {
                CHECKPOINT_CAMPAIGN_SOV1_OPENING_ARROWS
            }
            Self::CampaignOpening(movies::OpeningTarget::Unlocked) => {
                CHECKPOINT_CAMPAIGN_SOV1_OPENING_UNLOCKED
            }
            Self::CampaignOpening(movies::OpeningTarget::RealtimeUnlocked) => {
                "campaign-sov1-opening-realtime-unlocked"
            }
            Self::CampaignOpening(movies::OpeningTarget::RestoreUnlocked) => {
                CHECKPOINT_CAMPAIGN_SOV1_OPENING_RESTORE_UNLOCKED
            }
            Self::CampaignOpening(movies::OpeningTarget::LoadUnlockedWhileLocked) => {
                CHECKPOINT_CAMPAIGN_ALL1_OPENING_LOAD_UNLOCKED
            }
            Self::CampaignOpening(movies::OpeningTarget::AlliedTick(_)) => {
                "campaign-all1-opening-tick"
            }
            Self::CampaignOpening(movies::OpeningTarget::SovietTick(_)) => {
                "campaign-sov1-opening-tick"
            }
            Self::CampaignOpening(movies::OpeningTarget::SovietNavy(_)) => {
                "campaign-sov1-navy-tick"
            }
            Self::CampaignOpening(movies::OpeningTarget::SovietGroups(_)) => {
                "campaign-sov07-groups-tick"
            }
            Self::CampaignOpening(movies::OpeningTarget::SovietFortresses(_)) => {
                "campaign-sov02-fortresses-tick"
            }
            Self::CampaignOpening(movies::OpeningTarget::TimerRunning) => {
                "campaign-sov1-opening-timer-running"
            }
            Self::CampaignOpening(movies::OpeningTarget::TimerExpired) => {
                "campaign-sov1-opening-timer-expired"
            }
            Self::CampaignOpening(movies::OpeningTarget::RadarAlert) => "campaign-sov2-radar-alert",
            Self::CampaignOpening(movies::OpeningTarget::RadarRestored) => {
                "campaign-sov2-radar-restored"
            }
            Self::CampaignBackReturn => CHECKPOINT_CAMPAIGN_BACK_RETURN,
            Self::LoadSavedGame0xB7Steady => CHECKPOINT_LOAD_SAVED_GAME_0XB7_STEADY,
            Self::LoadSavedGame0xB7Entry(_) => "load-saved-game-0xb7-entry",
            Self::Options0xD5Steady => CHECKPOINT_OPTIONS_0XD5_STEADY,
            Self::OptionsVeraSteady => CHECKPOINT_OPTIONS_VERA_STEADY,
            Self::Lan(target) => {
                movies::LAN_CHECKPOINTS
                    .iter()
                    .find(|(_, candidate)| *candidate == target)
                    .expect("LAN checkpoint")
                    .0
            }
            Self::Wol(target) => {
                movies::WOL_CHECKPOINTS
                    .iter()
                    .find(|(_, candidate)| *candidate == target)
                    .expect("WOL checkpoint")
                    .0
            }
            Self::NetworkOptions(target) => {
                NETWORK_OPTIONS_CHECKPOINTS
                    .iter()
                    .find(|(_, candidate)| *candidate == target)
                    .expect("D7 checkpoint")
                    .0
            }
            Self::MainMenu0xE2NetworkBounce => CHECKPOINT_MAIN_MENU_0XE2_NETWORK_BOUNCE,
            Self::Wol0x10ESteady => CHECKPOINT_WOL_0X10E_STEADY,
            Self::Wol0x10EEntry(_) => "wol-0x10e-entry",
            Self::Wol0x10EHoverQuickMatch => CHECKPOINT_WOL_0X10E_HOVER_QUICK_MATCH,
            Self::Wol0x10EApiMissing => CHECKPOINT_WOL_0X10E_API_MISSING,
            Self::Wol0x10EBack => CHECKPOINT_WOL_0X10E_BACK,
            Self::Options0xD5Entry(_) => "options-0xd5-entry",
            Self::Options0xD5Hover(index) => OPTIONS_0XD5_HOVER_CHECKPOINTS[index].0,
            Self::CreditsRollFrame(_) => "credits-roll-frame",
            Self::SneakPeekFrame(_) => "sneak-peek-frame",
            Self::MainMenu0xE2SlideOut(_) => "main-menu-0xe2-slide-out",
            Self::MovieList0x129SlideOut(_) => "movie-list-0x129-slide-out",
            Self::Skirmish0x102BackSlideOut(_) => "skirmish-0x102-back-slide-out",
            Self::Skirmish0x102Entry(_) => "skirmish-0x102-entry",
            Self::Skirmish0x102Hover(index) => SKIRMISH_0X102_HOVER_CHECKPOINTS[index].0,
            Self::Skirmish0x105Steady => CHECKPOINT_SKIRMISH_0X105_STEADY,
            Self::Skirmish0x105Entry(_) => "skirmish-0x105-entry",
            Self::Skirmish0x105Hover(index) => SKIRMISH_0X105_HOVER_CHECKPOINTS[index].0,
            Self::Skirmish0x105CancelEntry(_) => "skirmish-0x105-cancel",
            Self::Skirmish0x105CancelSteady => CHECKPOINT_SKIRMISH_0X105_CANCEL_STEADY,
            Self::Skirmish0x105UseMap => CHECKPOINT_SKIRMISH_0X105_USE_MAP,
            Self::Skirmish0x105SeedBrowserReturn => CHECKPOINT_SKIRMISH_0X105_SEED_BROWSER_RETURN,
            Self::Skirmish0x105ListOpen => CHECKPOINT_SKIRMISH_0X105_LIST_OPEN,
            Self::Skirmish0x105EjectBox => CHECKPOINT_SKIRMISH_0X105_EJECT_BOX,
            Self::Skirmish0x105Eject(crate::ui::skirmish_shell::EjectPromptButton::Cancel) => {
                CHECKPOINT_SKIRMISH_0X105_EJECT_CANCEL
            }
            Self::Skirmish0x105Eject(crate::ui::skirmish_shell::EjectPromptButton::Ok) => {
                CHECKPOINT_SKIRMISH_0X105_EJECT_OK
            }
            Self::Skirmish0x6BSteady => CHECKPOINT_SKIRMISH_0X6B_STEADY,
            Self::Skirmish0x6BEntry(_) => "skirmish-0x6b-entry",
            Self::Skirmish0x102ChooseMapReturn => CHECKPOINT_SKIRMISH_0X102_CHOOSE_MAP_RETURN,
            Self::SkirmishStartBlank => CHECKPOINT_SKIRMISH_START_BLANK,
            Self::SkirmishLoadingFirstFrame => CHECKPOINT_SKIRMISH_LOADING_FIRST_FRAME,
            Self::Skirmish0x6BEjectBox => CHECKPOINT_SKIRMISH_0X6B_EJECT_BOX,
            Self::SkirmishAfterQuit => CHECKPOINT_SKIRMISH_AFTER_QUIT,
            Self::SkirmishDefeatScore => CHECKPOINT_SKIRMISH_DEFEAT_SCORE,
            Self::SkirmishDefeatContinue => CHECKPOINT_SKIRMISH_DEFEAT_CONTINUE,
            Self::SkirmishDiplomacy => CHECKPOINT_SKIRMISH_DIPLOMACY,
            Self::Score0x108Steady => CHECKPOINT_SCORE_0X108_STEADY,
            Self::CampaignScore0x103Steady => CHECKPOINT_CAMPAIGN_SCORE_0X103_STEADY,
            Self::CampaignScore0x103AnimationSequence => {
                CHECKPOINT_CAMPAIGN_SCORE_0X103_ANIMATION_SEQUENCE
            }
            Self::Score0x108Entry(_) => "score-0x108-entry",
            Self::Score0x108HoverContinue => CHECKPOINT_SCORE_0X108_HOVER_CONTINUE,
            Self::Score0x108LeaveContinue => CHECKPOINT_SCORE_0X108_LEAVE_CONTINUE,
            Self::Score0x108SlideOut(_) => "score-0x108-slide-out",
        }
    }

    fn movies_target(self) -> Option<movies::MoviesTarget> {
        Some(match self {
            Self::MoviesPage0x101Steady => movies::MoviesTarget::Page0x101,
            Self::MainMenu0xE2ExitConfirm => movies::MoviesTarget::ExitConfirm,
            Self::MovieList0x129Steady => movies::MoviesTarget::List0x129,
            Self::MovieList0x129Selected => movies::MoviesTarget::List0x129Selected,
            Self::MovieList0x129Full => movies::MoviesTarget::FullList { down_presses: 0 },
            Self::MovieList0x129FullDown2 => movies::MoviesTarget::FullList { down_presses: 2 },
            Self::MovieList0x129BackFirstFrame => movies::MoviesTarget::ListBackFirstFrame,
            Self::Campaign0x94Steady => movies::MoviesTarget::Campaign0x94 {
                press: None,
                hold_over: None,
                entry_tick: None,
            },
            Self::Campaign0x94SliderLeft => movies::MoviesTarget::Campaign0x94 {
                press: Some(CAMPAIGN_SLIDER_LEFT_POINT),
                hold_over: None,
                entry_tick: None,
            },
            Self::Campaign0x94SliderRight => movies::MoviesTarget::Campaign0x94 {
                press: Some(CAMPAIGN_SLIDER_RIGHT_POINT),
                hold_over: None,
                entry_tick: None,
            },
            Self::Campaign0x94SliderHeld => movies::MoviesTarget::Campaign0x94 {
                press: Some(CAMPAIGN_SLIDER_LEFT_POINT),
                hold_over: Some(CAMPAIGN_BACK_POINT),
                entry_tick: None,
            },
            Self::Campaign0x94Entry(tick) => movies::MoviesTarget::Campaign0x94 {
                press: None,
                hold_over: None,
                entry_tick: Some(tick),
            },
            Self::CampaignLoadingFirst(side) => movies::MoviesTarget::CampaignStart {
                side,
                target: movies::CampaignStartTarget::LoadingFirst,
            },
            Self::CampaignFirstLive(side) => movies::MoviesTarget::CampaignStart {
                side,
                target: movies::CampaignStartTarget::FirstLive,
            },
            Self::CampaignInitialSaved(side) => movies::MoviesTarget::CampaignStart {
                side,
                target: movies::CampaignStartTarget::InitialSaved,
            },
            Self::CampaignExplicitFirstLive(scenario) => movies::MoviesTarget::CampaignStart {
                side: if scenario.starts_with("ALL") {
                    crate::ui::campaign_shell::CampaignSide::Allied
                } else {
                    crate::ui::campaign_shell::CampaignSide::Soviet
                },
                target: movies::CampaignStartTarget::ExplicitFirstLive(scenario),
            },
            Self::CampaignAbortReturn(side) => movies::MoviesTarget::CampaignStart {
                side,
                target: movies::CampaignStartTarget::AbortReturn,
            },
            Self::CampaignRestart(
                target @ (movies::RestartTarget::ObjectiveExpiry
                | movies::RestartTarget::AlternateHomeVictory
                | movies::RestartTarget::InheritedTimerVictory),
            ) => movies::MoviesTarget::CampaignStart {
                side: crate::ui::campaign_shell::CampaignSide::Soviet,
                target: movies::CampaignStartTarget::Restart(target),
            },
            Self::CampaignRestart(target) => movies::MoviesTarget::CampaignStart {
                side: crate::ui::campaign_shell::CampaignSide::Allied,
                target: movies::CampaignStartTarget::Restart(target),
            },
            Self::CampaignOpening(target) => movies::MoviesTarget::CampaignStart {
                side: target.side(),
                target: movies::CampaignStartTarget::Opening(target),
            },
            Self::CampaignBackReturn => movies::MoviesTarget::CampaignBack,
            Self::LoadSavedGame0xB7Steady => {
                movies::MoviesTarget::LoadSavedGame0xB7 { entry_tick: None }
            }
            Self::LoadSavedGame0xB7Entry(tick) => movies::MoviesTarget::LoadSavedGame0xB7 {
                entry_tick: Some(tick),
            },
            Self::MainMenu0xE2NetworkBounce => movies::MoviesTarget::NetworkBounce,
            Self::Wol0x10ESteady => movies::MoviesTarget::WolWelcome {
                provider: crate::app::shell_wol::WolProvider::Unavailable,
                entry_tick: None,
                hover: None,
                press: movies::WolPress::None,
            },
            Self::Wol0x10EEntry(tick) => movies::MoviesTarget::WolWelcome {
                provider: crate::app::shell_wol::WolProvider::Unavailable,
                entry_tick: Some(tick),
                hover: None,
                press: movies::WolPress::None,
            },
            Self::Wol0x10EHoverQuickMatch => movies::MoviesTarget::WolWelcome {
                provider: crate::app::shell_wol::WolProvider::Unavailable,
                entry_tick: None,
                hover: Some(WOL_QUICK_MATCH_POINT),
                press: movies::WolPress::None,
            },
            Self::Wol0x10EApiMissing => movies::MoviesTarget::WolWelcome {
                provider: crate::app::shell_wol::WolProvider::Unavailable,
                entry_tick: None,
                hover: None,
                press: movies::WolPress::MyInformation,
            },
            Self::Wol0x10EBack => movies::MoviesTarget::WolWelcome {
                provider: crate::app::shell_wol::WolProvider::Unavailable,
                entry_tick: None,
                hover: None,
                press: movies::WolPress::MainMenu,
            },
            Self::NetworkOptions(target) => movies::MoviesTarget::NetworkOptions(target),
            Self::Lan(target) => movies::MoviesTarget::Lan(target),
            Self::Wol(target) => movies::MoviesTarget::Wol(target),
            Self::Options0xD5Steady => movies::MoviesTarget::Options0xD5 {
                entry_tick: None,
                hover: None,
                page: OptionsPage::Main,
            },
            Self::OptionsVeraSteady => movies::MoviesTarget::Options0xD5 {
                entry_tick: None,
                hover: None,
                page: OptionsPage::Vera,
            },
            Self::Options0xD5Entry(tick) => movies::MoviesTarget::Options0xD5 {
                entry_tick: Some(tick),
                hover: None,
                page: OptionsPage::Main,
            },
            Self::Options0xD5Hover(index) => movies::MoviesTarget::Options0xD5 {
                entry_tick: None,
                hover: Some(OPTIONS_0XD5_HOVER_CHECKPOINTS[index].1),
                page: OptionsPage::Main,
            },
            Self::CreditsRollFrame(frame) => movies::MoviesTarget::Credits { frame },
            Self::SneakPeekFrame(frame) => movies::MoviesTarget::SneakPeek { frame },
            Self::MainMenu0xE2SlideOut(tick) => movies::MoviesTarget::SlideOut {
                kind: ShellSlideKind::MainMenu,
                tick,
            },
            Self::MovieList0x129SlideOut(tick) => movies::MoviesTarget::SlideOut {
                kind: ShellSlideKind::MovieList,
                tick,
            },
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ShellCaptureRequest {
    checkpoint: ShellCaptureCheckpoint,
    width: u32,
    height: u32,
    cursor_x: u32,
    cursor_y: u32,
    output_dir: PathBuf,
    /// Diagnostic wall-time override; production session timers keep their owners.
    timeout: Option<Duration>,
    live_lan: Option<LiveLanConfig>,
    live_online: Option<LiveOnlineConfig>,
}

/// Nonsecret username and match inputs for an ordinary online UI run.
#[derive(Debug, Clone)]
pub(crate) struct LiveOnlineConfig {
    pub account: String,
    pub host: String,
    pub map: Option<String>,
    /// Real human/service sources; AI rows never become relay participants.
    pub players: usize,
    pub ai: Option<crate::skirmish_launch::AiDifficulty>,
    pub mode_id: Option<i32>,
    /// Authored waypoint index, independent of arrival-order source/raw row.
    pub start: Option<u8>,
    pub frames: u32,
    pub simple_input: bool,
    pub latency: bool,
    pub mass_input: bool,
    /// Public coordinator nonce; only diagnostic Observe input waits for ACK.
    pub network_checkpoint: Option<String>,
    /// Public nonce for the later exit/HTTPS-retry/reentry acceptance only.
    pub exit_checkpoint: Option<String>,
    /// External owned kill during first loading, followed by a normal two-player rematch.
    pub loading_host_loss: Option<String>,
    /// External owned kill after ordinary100, before the original host installs live gameplay.
    pub startup_host_loss: Option<String>,
    pub loading_output: Option<PathBuf>,
    pub loading_cancel: Option<LoadingCancelConfig>,
    pub observe_loading: bool,
    pub profile_check: bool,
    pub exit_prefix_output: Option<PathBuf>,
    /// Nonsecret real-command/checksum prefix, written before external faults.
    pub failure_prefix_output: Option<PathBuf>,
}

/// Diagnostic intent only; cancellation still runs the real service owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoadingCancelKind {
    Https,
    Departure,
}
impl LoadingCancelKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Https => "https",
            Self::Departure => "departure",
        }
    }
}
#[derive(Debug, Clone)]
pub(crate) struct LoadingCancelConfig {
    pub checkpoint_id: String,
    pub kind: LoadingCancelKind,
    pub output: PathBuf,
}

/// The victim is terminated externally by the owned-process runner. This
/// chooses only the capture checkpoint; the LAN service retains its real lease.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LanSetupCrash {
    Host,
    Guest,
    MapHost,
}

impl LanSetupCrash {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Host => "host",
            Self::Guest => "guest",
            Self::MapHost => "map-host",
        }
    }

    pub(crate) fn host(self) -> bool {
        matches!(self, Self::Host | Self::MapHost)
    }
}

/// Only socket routing differs between these diagnostic modes. Production
/// broadcast delegates all defaults to the ordinary LAN service owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LiveLanTransport {
    ProductionBroadcast,
    Explicit {
        bind: std::net::SocketAddrV4,
        peer: std::net::SocketAddrV4,
    },
}

impl LiveLanTransport {
    pub(crate) fn explicit_endpoints(
        self,
    ) -> Option<(std::net::SocketAddrV4, std::net::SocketAddrV4)> {
        match self {
            Self::ProductionBroadcast => None,
            Self::Explicit { bind, peer } => Some((bind, peer)),
        }
    }
}

/// Physical LAN capture inputs do not replace gameplay/session identity and
/// are unavailable to captures that inject their service transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LiveLanConfig {
    pub transport: LiveLanTransport,
    /// Host-only logical catalog name; RandMap.Sed drives the normal generator.
    pub map: Option<String>,
    /// Optional real pre-GO process-loss checkpoint. It never changes a timer.
    pub setup_crash: Option<LanSetupCrash>,
    pub profile_check: bool,
    /// Written before an external LAN proxy/process fault; contains no secrets.
    pub failure_prefix_output: Option<PathBuf>,
}

impl ShellCaptureRequest {
    /// Only explicit native diagnostics retain the original WOL surface and
    /// BD's Accept; the live LAN and online runs play as players do.
    pub(crate) fn native_shell_ui(&self) -> bool {
        match self.checkpoint.movies_target() {
            Some(movies::MoviesTarget::Wol(target)) => !matches!(
                target,
                movies::WolTarget::ModernLobby
                    | movies::WolTarget::ModernCreate
                    | movies::WolTarget::ModernRoomOptions
                    | movies::WolTarget::ModernSpectators
                    | movies::WolTarget::ModernOptions
                    | movies::WolTarget::ModernUsername
            ),
            Some(movies::MoviesTarget::WolWelcome {
                provider: crate::app::shell_wol::WolProvider::Unavailable,
                ..
            }) => true,
            Some(movies::MoviesTarget::Lan(target)) => !target.live(),
            _ => false,
        }
    }

    /// Select only the diagnostic visit; never change the saved profile.
    pub(crate) fn wol_welcome_provider(&self) -> Option<crate::app::shell_wol::WolProvider> {
        match self.checkpoint.movies_target() {
            Some(movies::MoviesTarget::WolWelcome { provider, .. }) => Some(provider),
            Some(movies::MoviesTarget::Wol(target)) => Some(target.welcome_provider()),
            _ => None,
        }
    }

    pub fn checkpoint(&self) -> ShellCaptureCheckpoint {
        self.checkpoint
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn cursor_x(&self) -> u32 {
        self.cursor_x
    }

    pub fn cursor_y(&self) -> u32 {
        self.cursor_y
    }

    pub fn output_dir(&self) -> &Path {
        &self.output_dir
    }

    /// Capture must start from the ordinary main menu. Existing developer
    /// shortcuts would silently bypass the checkpoint, so reject them.
    pub fn validate_runtime_environment(&self) -> Result<()> {
        ensure!(
            std::env::var_os("RA2_QUICKPLAY").is_none(),
            "RA2_QUICKPLAY must be unset for shell capture"
        );
        ensure!(
            !truthy_env("RA2_DEV_SKIRMISH_SHELL"),
            "RA2_DEV_SKIRMISH_SHELL must be unset or false for shell capture"
        );
        Ok(())
    }
}

fn public_coordinator_nonce(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Parse the normal/capture launch boundary without changing ordinary no-arg
/// startup. Capture mode is deliberately strict so a misspelled automation
/// command cannot open an interactive window.
pub fn parse_launch_args<I>(args: I) -> Result<AppLaunchMode>
where
    I: IntoIterator<Item = OsString>,
{
    let mut args = args.into_iter();
    let Some(first) = args.next() else {
        return Ok(AppLaunchMode::Interactive);
    };
    ensure!(
        first == CAPTURE_FLAG,
        "unknown argument {:?}; ordinary VERA20k launch takes no arguments",
        first
    );

    let checkpoint_raw = next_utf8(&mut args, "checkpoint after --shell-capture")?;
    let checkpoint = ShellCaptureCheckpoint::parse(&checkpoint_raw)?;
    let mut width = None;
    let mut height = None;
    let mut cursor_x = None;
    let mut cursor_y = None;
    let mut output_dir = None;
    let mut timeout = None;
    let mut lan_production_broadcast = false;
    let mut lan_bind = None;
    let mut lan_peer = None;
    let mut lan_map = None;
    let mut lan_setup_crash = None;
    let mut online_account = None;
    let mut online_host = None;
    let mut online_map = None;
    let mut online_players = None;
    let mut online_ai = None;
    let mut online_mode_id = None;
    let mut online_start = None;
    let mut online_frames = None;
    let mut online_simple = false;
    let mut online_latency = false;
    let mut online_mass = false;
    let mut online_network_checkpoint = None;
    let mut online_exit_checkpoint = None;
    let mut online_loading_host_loss = None;
    let mut online_startup_host_loss = None;
    let mut online_loading_cancel = None;
    let mut online_loading_cancel_checkpoint = None;
    let mut observe_loading = false;
    let mut profile_check = false;

    while let Some(flag) = args.next() {
        let flag = flag
            .into_string()
            .map_err(|_| anyhow::anyhow!("shell-capture option name is not valid UTF-8"))?;
        match flag.as_str() {
            "--width" => set_once(
                &mut width,
                parse_u32(next_utf8(&mut args, "value after --width")?, "--width")?,
                "--width",
            )?,
            "--height" => set_once(
                &mut height,
                parse_u32(next_utf8(&mut args, "value after --height")?, "--height")?,
                "--height",
            )?,
            "--cursor-x" => set_once(
                &mut cursor_x,
                parse_u32(
                    next_utf8(&mut args, "value after --cursor-x")?,
                    "--cursor-x",
                )?,
                "--cursor-x",
            )?,
            "--cursor-y" => set_once(
                &mut cursor_y,
                parse_u32(
                    next_utf8(&mut args, "value after --cursor-y")?,
                    "--cursor-y",
                )?,
                "--cursor-y",
            )?,
            "--output" => {
                let value = args
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("missing value after --output"))?;
                set_once(&mut output_dir, PathBuf::from(value), "--output")?;
            }
            "--shell-capture-timeout-seconds" => {
                let seconds = parse_u32(
                    next_utf8(&mut args, "value after --shell-capture-timeout-seconds")?,
                    "--shell-capture-timeout-seconds",
                )?;
                ensure!(
                    seconds > 0,
                    "--shell-capture-timeout-seconds must be greater than zero"
                );
                set_once(
                    &mut timeout,
                    Duration::from_secs(u64::from(seconds)),
                    "--shell-capture-timeout-seconds",
                )?;
            }
            "--lan-production-broadcast" => {
                ensure!(
                    !lan_production_broadcast,
                    "duplicate --lan-production-broadcast"
                );
                lan_production_broadcast = true;
            }
            "--lan-bind" => set_once(
                &mut lan_bind,
                next_utf8(&mut args, "value after --lan-bind")?
                    .parse::<std::net::SocketAddrV4>()
                    .context("--lan-bind requires an IPv4 address and port")?,
                "--lan-bind",
            )?,
            "--lan-peer" => set_once(
                &mut lan_peer,
                next_utf8(&mut args, "value after --lan-peer")?
                    .parse::<std::net::SocketAddrV4>()
                    .context("--lan-peer requires an IPv4 address and port")?,
                "--lan-peer",
            )?,
            "--lan-map" => set_once(
                &mut lan_map,
                next_utf8(&mut args, "value after --lan-map")?,
                "--lan-map",
            )?,
            "--lan-setup-crash" => {
                let victim = match next_utf8(&mut args, "value after --lan-setup-crash")?.as_str() {
                    "host" => LanSetupCrash::Host,
                    "guest" => LanSetupCrash::Guest,
                    "map-host" => LanSetupCrash::MapHost,
                    _ => bail!("--lan-setup-crash requires host, guest or map-host"),
                };
                set_once(&mut lan_setup_crash, victim, "--lan-setup-crash")?;
            }
            "--online-account" => set_once(
                &mut online_account,
                next_utf8(&mut args, "value after --online-account")?,
                "--online-account",
            )?,
            "--online-host" => set_once(
                &mut online_host,
                next_utf8(&mut args, "value after --online-host")?,
                "--online-host",
            )?,
            "--online-map" => set_once(
                &mut online_map,
                next_utf8(&mut args, "value after --online-map")?,
                "--online-map",
            )?,
            "--online-players" => set_once(
                &mut online_players,
                parse_u32(
                    next_utf8(&mut args, "value after --online-players")?,
                    "--online-players",
                )?,
                "--online-players",
            )?,
            "--online-ai" => {
                use crate::skirmish_launch::AiDifficulty;
                let difficulty = match next_utf8(&mut args, "value after --online-ai")?.as_str() {
                    "easy" => AiDifficulty::Easy,
                    "normal" => AiDifficulty::Normal,
                    "hard" => AiDifficulty::Hard,
                    _ => bail!("--online-ai requires easy, normal or hard"),
                };
                set_once(&mut online_ai, difficulty, "--online-ai")?;
            }
            "--online-mode-id" => set_once(
                &mut online_mode_id,
                i32::try_from(parse_u32(
                    next_utf8(&mut args, "value after --online-mode-id")?,
                    "--online-mode-id",
                )?)
                .context("--online-mode-id must fit a nonnegative i32")?,
                "--online-mode-id",
            )?,
            "--online-start" => set_once(
                &mut online_start,
                parse_u32(
                    next_utf8(&mut args, "value after --online-start")?,
                    "--online-start",
                )?,
                "--online-start",
            )?,
            "--online-frames" => set_once(
                &mut online_frames,
                parse_u32(
                    next_utf8(&mut args, "value after --online-frames")?,
                    "--online-frames",
                )?,
                "--online-frames",
            )?,
            "--online-mass-input" => {
                ensure!(!online_mass, "duplicate --online-mass-input");
                online_mass = true;
            }
            "--online-network-checkpoint" => set_once(
                &mut online_network_checkpoint,
                next_utf8(&mut args, "value after --online-network-checkpoint")?,
                "--online-network-checkpoint",
            )?,
            "--online-loading-host-loss" => set_once(
                &mut online_loading_host_loss,
                next_utf8(&mut args, "value after --online-loading-host-loss")?,
                "--online-loading-host-loss",
            )?,
            "--online-startup-host-loss" => set_once(
                &mut online_startup_host_loss,
                next_utf8(&mut args, "value after --online-startup-host-loss")?,
                "--online-startup-host-loss",
            )?,
            "--online-loading-cancel" => {
                let kind =
                    match next_utf8(&mut args, "value after --online-loading-cancel")?.as_str() {
                        "https" => LoadingCancelKind::Https,
                        "departure" => LoadingCancelKind::Departure,
                        _ => bail!("--online-loading-cancel requires https or departure"),
                    };
                set_once(&mut online_loading_cancel, kind, "--online-loading-cancel")?;
            }
            "--online-loading-cancel-checkpoint" => set_once(
                &mut online_loading_cancel_checkpoint,
                next_utf8(&mut args, "value after --online-loading-cancel-checkpoint")?,
                "--online-loading-cancel-checkpoint",
            )?,
            "--online-observe-loading" => {
                ensure!(!observe_loading, "duplicate --online-observe-loading");
                observe_loading = true;
            }
            "--network-profile-check" => {
                ensure!(!profile_check, "duplicate --network-profile-check");
                profile_check = true;
            }
            "--online-exit-checkpoint" => set_once(
                &mut online_exit_checkpoint,
                next_utf8(&mut args, "value after --online-exit-checkpoint")?,
                "--online-exit-checkpoint",
            )?,
            "--online-latency" => {
                ensure!(!online_latency, "duplicate --online-latency");
                online_latency = true;
            }
            "--online-simple-input" => {
                ensure!(!online_simple, "duplicate --online-simple-input");
                online_simple = true;
            }
            _ => bail!("unknown shell-capture option {flag:?}"),
        }
    }

    let width = width.context("missing required --width")?;
    let height = height.context("missing required --height")?;
    let cursor_x = cursor_x.context("missing required --cursor-x")?;
    let cursor_y = cursor_y.context("missing required --cursor-y")?;
    let output_dir = output_dir.context("missing required --output")?;

    ensure!(
        (width == EXPECTED_WIDTH && height == EXPECTED_HEIGHT)
            || (online_latency && width == 640 && height == 480),
        "checkpoint {} requires exactly {EXPECTED_WIDTH}x{EXPECTED_HEIGHT} (640x480 also allowed for --online-latency), got {width}x{height}",
        checkpoint.as_str()
    );
    ensure!(
        cursor_x == EXPECTED_CURSOR_X && cursor_y == EXPECTED_CURSOR_Y,
        "checkpoint {} requires neutral cursor ({EXPECTED_CURSOR_X},{EXPECTED_CURSOR_Y}), got \
         ({cursor_x},{cursor_y})",
        checkpoint.as_str()
    );
    validate_new_output_dir(&output_dir)?;
    let online = matches!(checkpoint, ShellCaptureCheckpoint::Lan(target) if target.online());
    let online_failure =
        matches!(checkpoint, ShellCaptureCheckpoint::Lan(target) if target.online_failure());
    let live = matches!(checkpoint, ShellCaptureCheckpoint::Lan(target) if target.live() && !target.online());
    if lan_production_broadcast {
        ensure!(
            matches!(
                checkpoint,
                ShellCaptureCheckpoint::Lan(
                    movies::LanTarget::LiveHost | movies::LanTarget::LiveGuest
                )
            ),
            "--lan-production-broadcast requires a normal LAN live host or guest checkpoint"
        );
        ensure!(
            lan_bind.is_none() && lan_peer.is_none() && lan_setup_crash.is_none(),
            "--lan-production-broadcast cannot override sockets or combine with a fault checkpoint"
        );
    }
    if profile_check {
        ensure!(
            matches!(
                checkpoint,
                ShellCaptureCheckpoint::Lan(
                    movies::LanTarget::OnlineHost | movies::LanTarget::LiveHost
                )
            ) && lan_setup_crash.is_none()
                && online_loading_host_loss.is_none()
                && online_startup_host_loss.is_none()
                && online_network_checkpoint.is_none()
                && online_exit_checkpoint.is_none()
                && online_loading_cancel.is_none(),
            "profile check requires a normal host checkpoint without another fault"
        );
        ensure!(
            if online {
                online_map.as_deref()
            } else {
                lan_map.as_deref()
            }
            .is_some_and(|file| !crate::map::rmg::is_seed_selection(file)),
            "profile check requires an authored30-capable catalog map"
        );
    }
    let live_online = if online {
        let account = online_account.context("online capture requires --online-account")?;
        ensure!(
            !matches!(checkpoint, ShellCaptureCheckpoint::Lan(target) if target.online_guest())
                || online_host.is_some(),
            "online guest capture requires --online-host"
        );
        let host = online_host.unwrap_or_else(|| account.clone());
        for name in [&account, &host] {
            ensure!(
                !name.is_empty()
                    && name.len() <= 24
                    && name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte)),
                "online account names must contain 1-24 letters, digits, underscores or hyphens"
            );
        }
        ensure!(
            online_start.is_none_or(
                |start| start < crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT as u32
            ),
            "online start index must be between0 and29"
        );
        let players = online_players.unwrap_or(2) as usize;
        ensure!(
            (1..=crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT).contains(&players)
                && (players >= 2 || online_ai.is_some()),
            "online players must be between2 and30, or one host with explicit --online-ai"
        );
        ensure!(
            !online_failure
                || (players == 2
                    && !online_mass
                    && online_frames.is_none_or(|frames| frames == 300)),
            "online failure capture requires two players, simple input and exactly300 prefix frames"
        );
        ensure!(
            !online_latency
                || (players == 2
                    && !online_failure
                    && !online_mass
                    && online_ai.is_none()
                    && !profile_check
                    && online_network_checkpoint.is_none()
                    && online_exit_checkpoint.is_none()
                    && online_loading_host_loss.is_none()
                    && online_startup_host_loss.is_none()
                    && online_loading_cancel.is_none()),
            "--online-latency requires a normal online pair without another diagnostic mode"
        );
        let frames = online_frames.unwrap_or(if online_latency {
            6000
        } else if online_failure {
            300
        } else if online_ai.is_some() {
            600
        } else if online_mass {
            1200
        } else if online_simple {
            300
        } else {
            7200
        });
        ensure!(
            (300..=1_000_000).contains(&frames),
            "online frame observation must cover300 to1000000 frames"
        );
        if online_ai.is_some() || online_mode_id.is_some() {
            ensure!(
                matches!(
                    checkpoint,
                    ShellCaptureCheckpoint::Lan(movies::LanTarget::OnlineHost)
                ) && account == host
                    && players == 1
                    && online_ai.is_some()
                    && frames >= 600
                    && !online_mass
                    && !profile_check
                    && online_network_checkpoint.is_none()
                    && online_exit_checkpoint.is_none()
                    && online_loading_host_loss.is_none()
                    && online_startup_host_loss.is_none()
                    && online_loading_cancel.is_none()
                    && online_loading_cancel_checkpoint.is_none(),
                "AI/mode capture requires one online host, explicit AI, at least600 frames and no separate fault/profile/mass case"
            );
        }
        ensure!(
            online_network_checkpoint.as_ref().is_none_or(|nonce| {
                !online_failure
                    && players == 2
                    && online_simple
                    && !online_mass
                    && frames >= 600
                    && public_coordinator_nonce(nonce)
            }),
            "network checkpoint requires a normal simple online pair, at least600 frames and a32-digit lowercase hex nonce"
        );
        ensure!(
            online_exit_checkpoint.as_ref().is_none_or(|nonce| {
                !online_failure
                    && online_network_checkpoint.is_none()
                    && players == 2
                    && online_simple
                    && !online_mass
                    && public_coordinator_nonce(nonce)
            }),
            "exit checkpoint requires a normal simple online pair and a32-digit lowercase hex nonce; other external faults are separate runs"
        );
        ensure!(
            online_loading_host_loss.as_ref().is_none_or(|nonce| {
                !online_failure
                    && online_network_checkpoint.is_none()
                    && online_exit_checkpoint.is_none()
                    && online_startup_host_loss.is_none()
                    && online_map.is_none()
                    && online_start.is_none()
                    && players == 3
                    && online_simple
                    && !online_mass
                    && frames >= 1024
                    && public_coordinator_nonce(nonce)
            }),
            "loading host loss requires a normal simple three-player online match, at least1024 frames and a32-digit lowercase hex nonce; map/start overrides and other faults are separate runs"
        );
        ensure!(
            online_startup_host_loss.as_ref().is_none_or(|nonce| {
                !online_failure
                    && online_loading_host_loss.is_none()
                    && online_network_checkpoint.is_none()
                    && online_exit_checkpoint.is_none()
                    && online_map.is_none()
                    && online_start.is_none()
                    && players == 3
                    && online_simple
                    && !online_mass
                    && public_coordinator_nonce(nonce)
            }),
            "startup host loss requires a normal simple three-player online match and a32-digit lowercase hex nonce; loading/map/start and other faults are separate runs"
        );
        ensure!(
            players <= 2 || online_simple || online_mass,
            "more than two players requires --online-simple-input or --online-mass-input"
        );
        ensure!(
            !online_map
                .as_deref()
                .is_some_and(|file| file.eq_ignore_ascii_case("RandMap.Sed"))
                || players <= 8,
            "random maps support two to eight players"
        );
        ensure!(
            online_loading_cancel.is_some() == online_loading_cancel_checkpoint.is_some(),
            "loading cancellation requires both kind and checkpoint nonce"
        );
        if let Some(nonce) = online_loading_cancel_checkpoint.as_deref() {
            ensure!(
                public_coordinator_nonce(nonce)
                    && players == 2
                    && online_simple
                    && !online_mass
                    && !online_failure
                    && online_loading_host_loss.is_none()
                    && online_startup_host_loss.is_none()
                    && online_network_checkpoint.is_none()
                    && online_exit_checkpoint.is_none()
                    && online_map.is_none()
                    && online_start.is_none()
                    && !profile_check,
                "loading cancellation requires a normal simple online pair and32 lowercase hex nonce without another fault/map/start"
            );
            for kind in ["ready", "control", "returned"] {
                ensure!(
                    !loading_runtime::sidecar(&output_dir, kind).try_exists()?,
                    "loading cancellation output already exists"
                );
            }
        }
        if online_loading_host_loss.is_some() {
            for kind in ["ready", "complete", "killed", "prefix", "removed"] {
                ensure!(
                    !loading_host_loss::sidecar(&output_dir, kind).try_exists()?,
                    "loading fault output already exists"
                );
            }
        }
        if online_startup_host_loss.is_some() {
            for kind in ["ready", "install", "prefix", "killed", "failure", "ack"] {
                ensure!(
                    !startup_host_loss::sidecar(&output_dir, kind).try_exists()?,
                    "startup fault output already exists"
                );
            }
        }
        Some(LiveOnlineConfig {
            account,
            host,
            map: online_map,
            players,
            ai: online_ai,
            mode_id: online_mode_id,
            start: online_start.map(|start| start as u8),
            frames,
            simple_input: online_simple
                || online_latency
                || online_mass
                || online_failure
                || online_ai.is_some(),
            latency: online_latency,
            mass_input: online_mass,
            failure_prefix_output: if online_failure || online_network_checkpoint.is_some() {
                let mut path = output_dir.as_os_str().to_owned();
                path.push(".prefix.json");
                let path = PathBuf::from(path);
                ensure!(!path.exists(), "failure prefix output already exists");
                Some(path)
            } else {
                None
            },
            network_checkpoint: online_network_checkpoint,
            exit_prefix_output: if online_exit_checkpoint.is_some() {
                let mut path = output_dir.as_os_str().to_owned();
                path.push(".exit.json");
                let path = PathBuf::from(path);
                ensure!(!path.exists(), "exit checkpoint output already exists");
                Some(path)
            } else {
                None
            },
            loading_cancel: online_loading_cancel.map(|kind| LoadingCancelConfig {
                checkpoint_id: online_loading_cancel_checkpoint
                    .expect("validated loading cancel nonce"),
                kind,
                output: output_dir.clone(),
            }),
            observe_loading,
            profile_check,
            exit_checkpoint: online_exit_checkpoint,
            loading_output: online_loading_host_loss
                .as_ref()
                .or(online_startup_host_loss.as_ref())
                .map(|_| output_dir.clone()),
            loading_host_loss: online_loading_host_loss,
            startup_host_loss: online_startup_host_loss,
        })
    } else {
        ensure!(
            online_account.is_none()
                && online_host.is_none()
                && online_map.is_none()
                && online_players.is_none()
                && online_ai.is_none()
                && online_mode_id.is_none()
                && online_start.is_none()
                && online_frames.is_none()
                && !online_simple
                && !online_latency
                && !online_mass
                && online_network_checkpoint.is_none()
                && online_exit_checkpoint.is_none()
                && online_loading_host_loss.is_none()
                && online_startup_host_loss.is_none()
                && online_loading_cancel.is_none()
                && online_loading_cancel_checkpoint.is_none()
                && !observe_loading,
            "online options require an online live checkpoint"
        );
        None
    };
    let live_lan = if live {
        let transport = if lan_production_broadcast {
            LiveLanTransport::ProductionBroadcast
        } else {
            let bind = lan_bind.context("live LAN capture requires --lan-bind")?;
            let peer = lan_peer.context("live LAN capture requires --lan-peer")?;
            ensure!(
                (1..=u16::MAX - 2).contains(&bind.port()),
                "--lan-bind needs a nonzero port with room for alternate port+2"
            );
            ensure!(
                !bind.ip().is_broadcast() && !bind.ip().is_multicast(),
                "--lan-bind is not a unicast bind address"
            );
            ensure!(
                peer.port() != 0
                    && !peer.ip().is_unspecified()
                    && !peer.ip().is_broadcast()
                    && !peer.ip().is_multicast(),
                "--lan-peer needs a unicast IPv4 endpoint"
            );
            ensure!(
                peer != bind && !(peer.ip() == bind.ip() && peer.port() == bind.port() + 2),
                "LAN peer overlaps a local socket"
            );
            LiveLanTransport::Explicit { bind, peer }
        };
        if let Some(file) = lan_map.as_deref() {
            ensure!(
                matches!(
                    checkpoint,
                    ShellCaptureCheckpoint::Lan(movies::LanTarget::LiveHost)
                ),
                "--lan-map requires the normal full-round LAN host checkpoint"
            );
            crate::map::source::validate_network_map_name(file)?;
        }
        ensure!(
            lan_setup_crash.is_none()
                || matches!(
                    checkpoint,
                    ShellCaptureCheckpoint::Lan(
                        movies::LanTarget::LiveHost | movies::LanTarget::LiveGuest
                    )
                ),
            "--lan-setup-crash requires a normal LAN live host or guest checkpoint"
        );
        ensure!(
            match lan_setup_crash {
                Some(LanSetupCrash::MapHost) => match checkpoint {
                    ShellCaptureCheckpoint::Lan(movies::LanTarget::LiveHost) => lan_map
                        .as_deref()
                        .is_some_and(|map| !crate::map::rmg::is_seed_selection(map)),
                    ShellCaptureCheckpoint::Lan(movies::LanTarget::LiveGuest) => lan_map.is_none(),
                    _ => false,
                },
                Some(_) => lan_map.is_none(),
                None => true,
            },
            "map-host requires an authored host map; resident host/guest crash uses the resident map"
        );
        let failure_prefix_output = if lan_setup_crash.is_some()
            || matches!(checkpoint, ShellCaptureCheckpoint::Lan(target) if target.desync_failure())
        {
            let mut path = output_dir.as_os_str().to_owned();
            path.push(".prefix.json");
            let path = PathBuf::from(path);
            ensure!(!path.exists(), "failure prefix output already exists");
            Some(path)
        } else {
            None
        };
        Some(LiveLanConfig {
            transport,
            map: lan_map,
            setup_crash: lan_setup_crash,
            profile_check,
            failure_prefix_output,
        })
    } else {
        ensure!(
            !lan_production_broadcast
                && lan_bind.is_none()
                && lan_peer.is_none()
                && lan_map.is_none()
                && lan_setup_crash.is_none(),
            "LAN socket options require a live LAN checkpoint"
        );
        None
    };

    Ok(AppLaunchMode::ShellCapture(ShellCaptureRequest {
        checkpoint,
        width,
        height,
        cursor_x,
        cursor_y,
        output_dir,
        timeout,
        live_lan,
        live_online,
    }))
}

fn next_utf8<I>(args: &mut I, what: &str) -> Result<String>
where
    I: Iterator<Item = OsString>,
{
    args.next()
        .ok_or_else(|| anyhow::anyhow!("missing {what}"))?
        .into_string()
        .map_err(|_| anyhow::anyhow!("{what} is not valid UTF-8"))
}

fn parse_u32(value: String, flag: &str) -> Result<u32> {
    value
        .parse()
        .with_context(|| format!("{flag} requires an unsigned integer, got {value:?}"))
}

fn set_once<T>(slot: &mut Option<T>, value: T, flag: &str) -> Result<()> {
    ensure!(slot.is_none(), "duplicate shell-capture option {flag}");
    *slot = Some(value);
    Ok(())
}

fn validate_new_output_dir(path: &Path) -> Result<()> {
    ensure!(path.is_absolute(), "--output must be an absolute path");
    ensure!(
        !path.exists(),
        "--output already exists; capture bundles are immutable: {}",
        path.display()
    );
    let parent = path
        .parent()
        .context("--output must have a parent directory")?;
    ensure!(
        parent.is_dir(),
        "--output parent does not exist or is not a directory: {}",
        parent.display()
    );
    Ok(())
}

fn truthy_env(name: &str) -> bool {
    std::env::var(name).ok().is_some_and(|value| {
        let value = value.trim();
        !value.is_empty()
            && value != "0"
            && !value.eq_ignore_ascii_case("false")
            && !value.eq_ignore_ascii_case("off")
            && !value.eq_ignore_ascii_case("no")
    })
}

#[derive(Debug, Clone, Copy)]
struct MainMenuCaptureSnapshot {
    width: u32,
    height: u32,
    main_menu_screen: bool,
    shell_failed: bool,
    single_player_active: bool,
    skirmish_active: bool,
    legacy_skirmish_setup_active: bool,
    modal_open: bool,
    quit_active: bool,
    first_paint_slide_active: bool,
    active_slide_is_main_menu: bool,
    title_terminal_persistent: bool,
    movie_loaded: bool,
    movie_owner_is_main_menu: bool,
    movie_base_is_large: bool,
    movie_base_matches_layout: bool,
    chrome_loaded: bool,
    software_cursor_active: bool,
    cursor_x: f32,
    cursor_y: f32,
}

impl MainMenuCaptureSnapshot {
    fn from_state(state: &AppState) -> Self {
        let movie_identity = state.frontend.main_menu_movie_identity;
        Self {
            width: state.renderer.gpu.config.width,
            height: state.renderer.gpu.config.height,
            main_menu_screen: state.frontend.screen == GameScreen::MainMenu,
            shell_failed: state.frontend.main_menu_shell_error.is_some(),
            single_player_active: state.frontend.shell_route.single_player(),
            skirmish_active: state.frontend.shell_route.skirmish(),
            // The legacy skirmish-setup flag was write-dead (never set after
            // startup); the capture snapshot keeps the field as literal false.
            legacy_skirmish_setup_active: false,
            modal_open: state.main_menu_dialog_open(),
            quit_active: state.frontend.quit_cascade.is_some(),
            first_paint_slide_active: state.frontend.shell_first_paint_slide.is_some(),
            active_slide_is_main_menu: state.frontend.shell_slide_active_shell
                == Some(ShellSlideKind::MainMenu),
            title_terminal_persistent: state
                .frontend
                .main_menu_shell_state
                .title_reveal
                .is_terminal_persistent(),
            movie_loaded: state.frontend.main_menu_movie.is_some(),
            movie_owner_is_main_menu: movie_identity
                .is_some_and(|identity| identity.owner() == Ra2tsDialogOwner::MainMenu0xE2),
            movie_base_is_large: movie_identity
                .is_some_and(|identity| identity.base() == MainMenuMovieBase::Ra2tsL),
            movie_base_matches_layout: movie_identity.is_some_and(|identity| {
                identity.base()
                    == crate::app::frontend::shell_layout::main_menu_layout(state).movie_base
            }),
            chrome_loaded: state.frontend.main_menu_shell_chrome.is_some(),
            software_cursor_active: state.use_software_cursor(),
            cursor_x: state.match_state.input.cursor_x,
            cursor_y: state.match_state.input.cursor_y,
        }
    }
}

fn steady_main_menu_capture_ready(snapshot: MainMenuCaptureSnapshot) -> Result<bool> {
    steady_main_menu_capture_ready_for(snapshot, false)
}

/// Live latency runs may use the normal 640x480 shell. Other captures retain
/// their fixed reference surface and large movie. The outer live capture also
/// checks that its actual surface equals the requested dimensions each frame.
fn steady_main_menu_capture_ready_for(
    snapshot: MainMenuCaptureSnapshot,
    latency: bool,
) -> Result<bool> {
    ensure!(
        (snapshot.width == EXPECTED_WIDTH && snapshot.height == EXPECTED_HEIGHT)
            || (latency && (snapshot.width, snapshot.height) == (640, 480)),
        "capture surface is outside its allowed dimensions: {}x{}",
        snapshot.width,
        snapshot.height
    );
    ensure!(
        snapshot.main_menu_screen,
        "capture left the main-menu screen before the checkpoint"
    );
    ensure!(!snapshot.shell_failed, "native main-menu shell fell back");
    ensure!(
        !snapshot.single_player_active
            && !snapshot.skirmish_active
            && !snapshot.legacy_skirmish_setup_active,
        "capture is not on bare dialog 0xE2"
    );
    ensure!(
        !snapshot.modal_open && !snapshot.quit_active,
        "capture cannot run with a modal or quit cascade active"
    );
    ensure!(
        snapshot.cursor_x == EXPECTED_CURSOR_X as f32
            && snapshot.cursor_y == EXPECTED_CURSOR_Y as f32,
        "capture cursor moved from the sealed neutral point"
    );

    if snapshot.first_paint_slide_active {
        return Ok(false);
    }

    ensure!(
        snapshot.active_slide_is_main_menu,
        "main-menu first-paint lifecycle did not settle on dialog 0xE2"
    );
    ensure!(
        snapshot.movie_loaded
            && snapshot.movie_owner_is_main_menu
            && if latency {
                snapshot.movie_base_matches_layout
            } else {
                snapshot.movie_base_is_large
            },
        "main-menu movie session identity is not ready"
    );
    ensure!(
        snapshot.chrome_loaded,
        "main-menu shell chrome is unavailable"
    );
    ensure!(
        snapshot.software_cursor_active,
        "retail software cursor is unavailable"
    );
    if !snapshot.title_terminal_persistent {
        return Ok(false);
    }
    Ok(true)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EntrySequenceFrameIdentity {
    generation: u64,
    tick: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShellCompletionHandoff {
    Continue,
    FinalizeExitReturnBeforeAcquire,
}

struct PendingSequenceReadback {
    identity: EntrySequenceFrameIdentity,
    readback: PendingBgra8Readback,
    submission: wgpu::SubmissionIndex,
}

#[derive(Default)]
struct EntrySequenceState {
    expected_next_tick: u8,
    generation: Option<u64>,
    pending: Vec<PendingSequenceReadback>,
    completion_observed: bool,
}

impl EntrySequenceState {
    fn validate_next(&self, identity: EntrySequenceFrameIdentity) -> Result<()> {
        ensure!(
            self.pending.len() < usize::from(ENTRY_SEQUENCE_FRAMES),
            "entry sequence attempted a frame past tick {ENTRY_SEQUENCE_TERMINAL_TICK}"
        );
        ensure!(
            identity.tick == self.expected_next_tick,
            "entry sequence tick gap/duplicate: expected {}, got {}",
            self.expected_next_tick,
            identity.tick
        );
        ensure!(
            identity.tick <= ENTRY_SEQUENCE_TERMINAL_TICK,
            "entry sequence tick {} exceeds terminal tick",
            identity.tick
        );
        if let Some(generation) = self.generation {
            ensure!(
                identity.generation == generation,
                "entry sequence generation changed from {generation} to {}",
                identity.generation
            );
        }
        Ok(())
    }

    fn record(
        &mut self,
        identity: EntrySequenceFrameIdentity,
        readback: PendingBgra8Readback,
        submission: wgpu::SubmissionIndex,
    ) -> Result<()> {
        self.validate_next(identity)?;
        self.generation.get_or_insert(identity.generation);
        self.expected_next_tick = self.expected_next_tick.saturating_add(1);
        self.pending.push(PendingSequenceReadback {
            identity,
            readback,
            submission,
        });
        Ok(())
    }
}

pub(crate) struct ShellCaptureSession {
    request: ShellCaptureRequest,
    started_at: Option<Instant>,
    frames_seen: u32,
    readback_started: bool,
    entry_sequence: Option<EntrySequenceState>,
    skirmish: Option<skirmish::SkirmishCapture>,
    movies: Option<movies::MoviesCapture>,
    wol_welcome_provider_selected: bool,
    score: Option<score::ScoreCapture>,
    campaign_score: Option<score::CampaignScoreCapture>,
    outcome: Option<std::result::Result<(), String>>,
}

impl ShellCaptureSession {
    pub(crate) fn new(request: ShellCaptureRequest) -> Self {
        let entry_sequence = (request.checkpoint
            == ShellCaptureCheckpoint::MainMenu0xE2EntrySequence)
            .then(EntrySequenceState::default);
        let skirmish = match request.checkpoint {
            ShellCaptureCheckpoint::Skirmish0x102Steady => {
                Some(skirmish::SkirmishCapture::default())
            }
            ShellCaptureCheckpoint::Skirmish0x102BackSlideOut(tick) => {
                Some(skirmish::SkirmishCapture::slide_out(tick))
            }
            ShellCaptureCheckpoint::Skirmish0x6BSteady => Some(skirmish::SkirmishCapture::chooser(
                skirmish::ChooserTarget::Steady,
            )),
            ShellCaptureCheckpoint::Skirmish0x6BEntry(tick) => Some(
                skirmish::SkirmishCapture::chooser(skirmish::ChooserTarget::Entry(tick)),
            ),
            ShellCaptureCheckpoint::Skirmish0x102ChooseMapReturn => Some(
                skirmish::SkirmishCapture::chooser(skirmish::ChooserTarget::Return),
            ),
            ShellCaptureCheckpoint::SkirmishStartBlank => Some(skirmish::SkirmishCapture::loading(
                skirmish::LoadingTarget::Blank,
            )),
            ShellCaptureCheckpoint::SkirmishLoadingFirstFrame => Some(
                skirmish::SkirmishCapture::loading(skirmish::LoadingTarget::FirstFrame),
            ),
            ShellCaptureCheckpoint::Skirmish0x6BEjectBox => Some(
                skirmish::SkirmishCapture::chooser(skirmish::ChooserTarget::Eject),
            ),
            ShellCaptureCheckpoint::SkirmishAfterQuit => Some(skirmish::SkirmishCapture::loading(
                skirmish::LoadingTarget::AfterQuit,
            )),
            ShellCaptureCheckpoint::SkirmishDefeatScore => Some(
                skirmish::SkirmishCapture::loading(skirmish::LoadingTarget::DefeatScore),
            ),
            ShellCaptureCheckpoint::SkirmishDefeatContinue => Some(
                skirmish::SkirmishCapture::loading(skirmish::LoadingTarget::DefeatContinue),
            ),
            ShellCaptureCheckpoint::SkirmishDiplomacy => Some(skirmish::SkirmishCapture::loading(
                skirmish::LoadingTarget::Diplomacy,
            )),
            ShellCaptureCheckpoint::Skirmish0x102Entry(tick) => {
                Some(skirmish::SkirmishCapture::entry(tick))
            }
            ShellCaptureCheckpoint::Skirmish0x102Hover(index) => Some(
                skirmish::SkirmishCapture::hover(SKIRMISH_0X102_HOVER_CHECKPOINTS[index].1),
            ),
            ShellCaptureCheckpoint::Skirmish0x105Steady => Some(
                skirmish::SkirmishCapture::random_map(skirmish::RandomMapTarget::Steady),
            ),
            ShellCaptureCheckpoint::Skirmish0x105Entry(tick) => Some(
                skirmish::SkirmishCapture::random_map(skirmish::RandomMapTarget::Entry(tick)),
            ),
            ShellCaptureCheckpoint::Skirmish0x105Hover(index) => {
                Some(skirmish::SkirmishCapture::random_map(
                    skirmish::RandomMapTarget::Hover(SKIRMISH_0X105_HOVER_CHECKPOINTS[index].1),
                ))
            }
            ShellCaptureCheckpoint::Skirmish0x105CancelEntry(tick) => Some(
                skirmish::SkirmishCapture::random_map(skirmish::RandomMapTarget::CancelEntry(tick)),
            ),
            ShellCaptureCheckpoint::Skirmish0x105CancelSteady => Some(
                skirmish::SkirmishCapture::random_map(skirmish::RandomMapTarget::CancelSteady),
            ),
            ShellCaptureCheckpoint::Skirmish0x105UseMap => Some(
                skirmish::SkirmishCapture::random_map(skirmish::RandomMapTarget::UseMap),
            ),
            ShellCaptureCheckpoint::Skirmish0x105SeedBrowserReturn => Some(
                skirmish::SkirmishCapture::random_map(skirmish::RandomMapTarget::SeedBrowserReturn),
            ),
            ShellCaptureCheckpoint::Skirmish0x105ListOpen => Some(
                skirmish::SkirmishCapture::random_map(skirmish::RandomMapTarget::ListOpen),
            ),
            ShellCaptureCheckpoint::Skirmish0x105EjectBox => Some(
                skirmish::SkirmishCapture::random_map(skirmish::RandomMapTarget::EjectBox),
            ),
            ShellCaptureCheckpoint::Skirmish0x105Eject(answer) => Some(
                skirmish::SkirmishCapture::random_map(skirmish::RandomMapTarget::Eject(answer)),
            ),
            _ => None,
        };
        let movies = request.checkpoint.movies_target().map(|target| {
            movies::MoviesCapture::new(
                target,
                request.output_dir(),
                request.live_lan.clone(),
                request.live_online.clone(),
            )
        });
        let score = match request.checkpoint {
            ShellCaptureCheckpoint::Score0x108Steady => Some(score::ScoreTarget::Steady),
            ShellCaptureCheckpoint::Score0x108Entry(tick) => Some(score::ScoreTarget::Entry(tick)),
            ShellCaptureCheckpoint::Score0x108HoverContinue => {
                Some(score::ScoreTarget::HoverContinue)
            }
            ShellCaptureCheckpoint::Score0x108LeaveContinue => {
                Some(score::ScoreTarget::LeaveContinue)
            }
            ShellCaptureCheckpoint::Score0x108SlideOut(tick) => {
                Some(score::ScoreTarget::SlideOut(tick))
            }
            _ => None,
        }
        .map(score::ScoreCapture::new);
        let campaign_score = match request.checkpoint {
            ShellCaptureCheckpoint::CampaignScore0x103Steady => {
                Some(score::CampaignScoreCapture::new())
            }
            ShellCaptureCheckpoint::CampaignScore0x103AnimationSequence => {
                Some(score::CampaignScoreCapture::animation_sequence())
            }
            _ => None,
        };
        Self {
            request,
            started_at: None,
            frames_seen: 0,
            readback_started: false,
            entry_sequence,
            skirmish,
            movies,
            wol_welcome_provider_selected: false,
            score,
            campaign_score,
            outcome: None,
        }
    }

    pub(crate) fn request(&self) -> &ShellCaptureRequest {
        &self.request
    }

    pub(crate) fn is_entry_sequence(&self) -> bool {
        self.entry_sequence.is_some()
    }

    /// A capture frame completes the command only when this state is terminal.
    /// Ordinary stills become terminal after their first readback; the campaign
    /// animation sequence retains the event loop between its three samples.
    pub(crate) fn is_terminal(&self) -> bool {
        self.outcome.is_some()
    }

    pub(crate) fn completion_handoff(&self) -> ShellCompletionHandoff {
        if self.is_entry_sequence() {
            ShellCompletionHandoff::FinalizeExitReturnBeforeAcquire
        } else {
            ShellCompletionHandoff::Continue
        }
    }

    /// First-live observes tick zero; opening captures drive exact steps through
    /// the same production runtime. Abort-return releases this ownership after
    /// its first draw so the ordinary modal/EXIT owners govern remaining frames.
    pub(crate) fn owns_simulation_steps(&self, state: &AppState) -> bool {
        self.movies
            .as_ref()
            .is_some_and(|capture| capture.owns_simulation_steps(state))
    }

    pub(crate) fn prepare_state(&mut self, state: &mut AppState) {
        state.match_state.input.cursor_x = self.request.cursor_x as f32;
        state.match_state.input.cursor_y = self.request.cursor_y as f32;
        state.frontend.native_shell_ui = self.request.native_shell_ui();
        self.started_at = Some(Instant::now());
    }

    fn timeout(&self) -> Duration {
        if let Some(timeout) = self.request.timeout {
            return timeout;
        }
        if let Some(capture) = &self.skirmish {
            capture.timeout()
        } else if let Some(capture) = &self.movies {
            capture.timeout()
        } else if self.score.is_some() || self.campaign_score.is_some() {
            Duration::from_secs(60)
        } else {
            CAPTURE_TIMEOUT
        }
    }

    fn max_capture_frames(&self) -> u32 {
        if self.request.live_lan.is_none() && self.request.live_online.is_none() {
            // Campaign movies/menu frames precede the requested simulation tick.
            // Keep that presentation overhead separate from the bounded play run.
            return match self.request.checkpoint {
                ShellCaptureCheckpoint::CampaignOpening(
                    movies::OpeningTarget::SovietTick(tick)
                    | movies::OpeningTarget::SovietNavy(tick)
                    | movies::OpeningTarget::SovietFortresses(tick),
                ) => MAX_CAPTURE_FRAMES + tick as u32,
                _ => MAX_CAPTURE_FRAMES,
            };
        }
        // Live matches keep production pacing and wait on real peers. Their
        // multi-round wall-time budget, not a still-frame limit, bounds the
        // nominal 8 ms capture loop. Retain the still-frame allowance for UI
        // and event-driven wakes; the wall deadline remains independently enforced.
        let frames = self
            .timeout()
            .as_nanos()
            .div_ceil(CAPTURE_FRAME_INTERVAL.as_nanos());
        u32::try_from(frames)
            .unwrap_or(u32::MAX)
            .saturating_add(MAX_CAPTURE_FRAMES)
    }

    fn is_online_latency(&self) -> bool {
        self.request
            .live_online
            .as_ref()
            .is_some_and(|config| config.latency)
    }

    fn check_capture_budget(&self, elapsed: Duration) -> Result<()> {
        // Latency uses the ordinary immediate redraw cadence: render count
        // no longer estimates wall time. Its existing wall deadline remains
        // mandatory even if a fast GPU produces many more than 125 draws/sec.
        let max_frames = self.max_capture_frames();
        ensure!(
            self.is_online_latency() || self.frames_seen <= max_frames,
            "shell capture exceeded {max_frames} frames"
        );
        ensure!(
            elapsed <= self.timeout(),
            "shell capture timed out after {} seconds",
            self.timeout().as_secs()
        );
        Ok(())
    }

    fn steady_ready(&self, state: &AppState) -> Result<bool> {
        if let Some(capture) = &self.movies {
            return capture.ready(state);
        }
        if let Some(capture) = &self.score {
            return capture.ready(state);
        }
        if let Some(capture) = &self.campaign_score {
            return capture.ready(state);
        }
        match &self.skirmish {
            Some(capture) => capture.ready(state),
            None => steady_main_menu_capture_ready(MainMenuCaptureSnapshot::from_state(state)),
        }
    }

    pub(crate) fn after_present(
        &mut self,
        state: &mut AppState,
        rendered: PresentedShell,
    ) -> Result<()> {
        if self.is_online_latency() {
            ensure!(
                (
                    state.renderer.gpu.config.width,
                    state.renderer.gpu.config.height
                ) == (self.request.width, self.request.height),
                "latency capture surface changed from the requested dimensions"
            );
        }
        // A native visit opened with the retail welcome (`prepare_state`);
        // the normal online visit has none and needs no selection.
        if let Some(visit) = state.frontend.wol.as_mut()
            && !self.wol_welcome_provider_selected
            && matches!(visit.page, crate::app::shell_wol::WolPage::Welcome)
            && let Some(provider) = self.request.wol_welcome_provider()
        {
            visit.set_provider(provider);
            self.wol_welcome_provider_selected = true;
            // Normal Online entry skips native persona I/O. Only after the
            // explicit native diagnostic selection may its existing owner
            // prefill and reopen the welcome.
            crate::app::shell_wol::persona::enter_wol_main(state);
            crate::app::App::reopen_wol_welcome(state);
            // The ordinary reducer rearms the selected page's entry before
            // any controlled driver reads its controls. Native fixtures are
            // confined to the capture's separately marked profile.
            crate::app::frontend::shell_transition::invalidate_main_menu_dialog_instance(state);
            return Ok(());
        }
        if let Some(capture) = &mut self.skirmish {
            capture.after_present(state, rendered, self.frames_seen)?;
        }
        if let Some(capture) = &mut self.movies {
            capture.after_present(state, rendered, self.frames_seen)?;
        }
        if let Some(capture) = &mut self.score {
            capture.after_present(state, rendered, self.frames_seen)?;
        }
        if let Some(capture) = &mut self.campaign_score {
            capture.after_present(state, rendered, self.frames_seen)?;
        }
        Ok(())
    }

    pub(crate) fn should_capture_current_frame(&mut self, state: &AppState) -> Result<bool> {
        ensure!(
            self.outcome.is_none(),
            "shell capture attempted after its outcome was recorded"
        );
        let started_at = self
            .started_at
            .context("shell capture was not initialized")?;
        self.frames_seen = self.frames_seen.saturating_add(1);
        self.check_capture_budget(started_at.elapsed())?;

        if self.is_entry_sequence() {
            return Ok(false);
        }
        if self
            .campaign_score
            .as_ref()
            .is_some_and(score::CampaignScoreCapture::is_animation_sequence)
        {
            return Ok(false);
        }

        ensure!(
            !self.readback_started,
            "shell capture attempted more than one readback"
        );
        let ready = self.steady_ready(state)?;
        if ready {
            self.readback_started = true;
        }
        Ok(ready)
    }

    /// Animation samples are selected after the production renderer has
    /// polled and drawn its page-local SCRA frame, so the stored index matches
    /// the final swapchain bytes submitted for readback.
    pub(crate) fn should_capture_after_render(
        &mut self,
        state: &AppState,
        rendered: PresentedShell,
    ) -> Result<bool> {
        let Some(capture) = self
            .campaign_score
            .as_mut()
            .filter(|capture| capture.is_animation_sequence())
        else {
            return Ok(false);
        };
        ensure!(
            !self.readback_started,
            "campaign animation sequence requested overlapping readbacks"
        );
        let should_capture =
            capture.should_capture_animation_frame(state, rendered, self.frames_seen)?;
        self.readback_started = should_capture;
        Ok(should_capture)
    }

    pub(crate) fn observe_entry_sequence_after_render(
        &mut self,
        state: &AppState,
        token: &MainMenuEntryPresentToken,
    ) -> Result<Option<EntrySequenceFrameIdentity>> {
        let Some(sequence) = self.entry_sequence.as_ref() else {
            return Ok(None);
        };
        ensure!(
            self.outcome.is_none(),
            "entry sequence already has an outcome"
        );
        let started_at = self
            .started_at
            .context("shell capture was not initialized")?;
        ensure!(
            started_at.elapsed() <= self.timeout(),
            "shell capture timed out after {} seconds",
            self.timeout().as_secs()
        );

        let snapshot = MainMenuCaptureSnapshot::from_state(state);
        ensure!(
            snapshot.width == EXPECTED_WIDTH && snapshot.height == EXPECTED_HEIGHT,
            "entry sequence surface changed from 800x600"
        );
        ensure!(
            snapshot.main_menu_screen,
            "entry sequence left the main menu"
        );
        ensure!(!snapshot.shell_failed, "native main-menu shell fell back");
        ensure!(
            !snapshot.single_player_active
                && !snapshot.skirmish_active
                && !snapshot.legacy_skirmish_setup_active,
            "entry sequence is not on bare dialog 0xE2"
        );
        ensure!(
            !snapshot.modal_open && !snapshot.quit_active,
            "entry sequence cannot run with a modal or quit cascade"
        );
        ensure!(
            snapshot.cursor_x == EXPECTED_CURSOR_X as f32
                && snapshot.cursor_y == EXPECTED_CURSOR_Y as f32,
            "entry sequence cursor moved from the sealed neutral point"
        );
        ensure!(
            snapshot.first_paint_slide_active && snapshot.active_slide_is_main_menu,
            "entry sequence lost the active 0xE2 wave"
        );
        ensure!(
            snapshot.movie_loaded
                && snapshot.movie_owner_is_main_menu
                && snapshot.movie_base_is_large
                && snapshot.chrome_loaded
                && snapshot.software_cursor_active,
            "entry sequence production shell identity is incomplete"
        );
        ensure!(
            matches!(
                state
                    .frontend
                    .main_menu_shell_state
                    .title_reveal
                    .paint_window(),
                Kind1PaintWindow::Hidden
            ),
            "entry sequence title became visible"
        );
        let frame = crate::app::frontend::shell_transition::current_main_menu_entry_frame(state)
            .context("entry sequence rendered without a ready frame")?;
        let identity = EntrySequenceFrameIdentity {
            generation: token.generation(),
            tick: token.tick(),
        };
        ensure!(
            (frame.generation(), frame.tick()) == (identity.generation, identity.tick),
            "entry sequence token does not match the rendered wave frame"
        );
        sequence.validate_next(identity)?;
        Ok(Some(identity))
    }

    pub(crate) fn record_entry_sequence_submission(
        &mut self,
        identity: EntrySequenceFrameIdentity,
        readback: PendingBgra8Readback,
        submission: wgpu::SubmissionIndex,
    ) -> Result<()> {
        self.entry_sequence
            .as_mut()
            .context("entry-sequence submission used for steady capture")?
            .record(identity, readback, submission)
    }

    pub(crate) fn readback_timeout(&self) -> Result<Duration> {
        let started_at = self
            .started_at
            .context("shell capture was not initialized")?;
        let remaining = self
            .timeout()
            .checked_sub(started_at.elapsed())
            .context("shell capture timeout expired before GPU readback")?;
        ensure!(
            !remaining.is_zero(),
            "shell capture timeout expired before GPU readback"
        );
        Ok(remaining)
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.outcome.is_some()
    }

    pub(crate) fn next_wake_deadline(&self) -> Instant {
        self.next_wake_deadline_at(Instant::now())
    }

    fn next_wake_deadline_at(&self, now: Instant) -> Instant {
        if self.is_online_latency() {
            now
        } else {
            now + CAPTURE_FRAME_INTERVAL
        }
    }

    pub(crate) fn complete(
        &mut self,
        state: &AppState,
        surface_format: wgpu::TextureFormat,
        pixels: &[u8],
    ) -> Result<()> {
        if self
            .campaign_score
            .as_ref()
            .is_some_and(score::CampaignScoreCapture::is_animation_sequence)
        {
            ensure!(
                self.readback_started,
                "campaign sequence completed without an outstanding readback"
            );
            let expected_len = usize::try_from(
                u64::from(self.request.width) * u64::from(self.request.height) * u64::from(4_u8),
            )
            .context("campaign sequence byte length does not fit usize")?;
            ensure!(
                pixels.len() == expected_len,
                "campaign sequence BGRA8 frame length mismatch: expected {expected_len}, got {}",
                pixels.len()
            );

            let final_bundle = {
                let capture = self
                    .campaign_score
                    .as_mut()
                    .expect("animation sequence has a campaign capture");
                capture.record_animation_sample(
                    state,
                    self.request.width,
                    self.request.height,
                    pixels,
                )?;
                if capture.animation_sequence_complete() {
                    let sample_byte_length = capture
                        .animation_sample_byte_length(self.request.width, self.request.height);
                    Some((
                        capture.animation_payload(),
                        capture.animation_manifest(
                            &self.request,
                            surface_format,
                            sample_byte_length,
                        ),
                        capture.animation_png_frames(self.request.width, self.request.height)?,
                    ))
                } else {
                    None
                }
            };
            self.readback_started = false;
            if let Some((payload, manifest, png_frames)) = final_bundle {
                let mut manifest_bytes = serde_json::to_vec_pretty(&manifest)
                    .context("serialize campaign animation capture manifest")?;
                manifest_bytes.push(b'\n');
                fs::create_dir(self.request.output_dir()).with_context(|| {
                    format!(
                        "create immutable campaign animation capture directory {}",
                        self.request.output_dir().display()
                    )
                })?;
                write_new_file(&self.request.output_dir().join("frames.bgra"), &payload)?;
                for (name, png) in png_frames {
                    write_new_file(&self.request.output_dir().join(name), &png)?;
                }
                write_new_file(
                    &self.request.output_dir().join(MANIFEST_FILE_NAME),
                    &manifest_bytes,
                )?;
                self.outcome = Some(Ok(()));
            }
            return Ok(());
        }

        ensure!(
            self.readback_started,
            "capture completed before readback started"
        );
        ensure!(
            self.outcome.is_none(),
            "capture outcome was already recorded"
        );

        let expected_len = usize::try_from(
            u64::from(self.request.width) * u64::from(self.request.height) * u64::from(4_u8),
        )
        .context("capture byte length does not fit usize")?;
        ensure!(
            pixels.len() == expected_len,
            "tight BGRA8 frame length mismatch: expected {expected_len}, got {}",
            pixels.len()
        );
        ensure!(
            self.steady_ready(state)?,
            "capture state changed before bundle write"
        );

        let mut manifest_bytes = match (
            &self.skirmish,
            &self.movies,
            &self.score,
            &self.campaign_score,
        ) {
            (Some(capture), _, _, _) => serde_json::to_vec_pretty(&capture.manifest(
                &self.request,
                surface_format,
                pixels,
                self.frames_seen,
            )),
            (None, Some(capture), _, _) => serde_json::to_vec_pretty(&capture.manifest(
                &self.request,
                surface_format,
                pixels,
                self.frames_seen,
                state,
            )?),
            (None, None, Some(capture), _) => serde_json::to_vec_pretty(&capture.manifest(
                &self.request,
                surface_format,
                pixels,
                self.frames_seen,
            )),
            (None, None, None, Some(capture)) => serde_json::to_vec_pretty(&capture.manifest(
                &self.request,
                surface_format,
                pixels,
                self.frames_seen,
            )),
            (None, None, None, None) => serde_json::to_vec_pretty(&capture_manifest(
                &self.request,
                surface_format,
                pixels.len() as u64,
            )),
        }
        .context("serialize shell-capture manifest")?;
        manifest_bytes.push(b'\n');
        fs::create_dir(self.request.output_dir()).with_context(|| {
            format!(
                "create immutable shell-capture directory {}",
                self.request.output_dir().display()
            )
        })?;
        let frame_path = self.request.output_dir().join(FRAME_FILE_NAME);
        write_new_file(&frame_path, pixels)?;
        let manifest_path = self.request.output_dir().join(MANIFEST_FILE_NAME);
        write_new_file(&manifest_path, &manifest_bytes)?;
        if let Some(capture) = self.movies.as_mut() {
            capture.cleanup()?;
        }

        // The bundle keeps every partial observation, but an incomplete
        // measurement must not exit as a successful capture.
        self.outcome = Some(
            match self
                .movies
                .as_ref()
                .and_then(movies::MoviesCapture::measurement_failure)
            {
                Some(failure) => Err(failure),
                None => Ok(()),
            },
        );
        Ok(())
    }

    pub(crate) fn complete_entry_sequence_after_wave(&mut self, state: &AppState) -> Result<()> {
        ensure!(
            self.request.checkpoint == ShellCaptureCheckpoint::MainMenu0xE2EntrySequence,
            "entry-sequence completion used for steady capture"
        );
        ensure!(
            self.outcome.is_none(),
            "capture outcome was already recorded"
        );
        ensure!(
            state.frontend.shell_first_paint_slide.is_none(),
            "entry sequence completion ran before the wave cleared"
        );
        let started_at = self
            .started_at
            .context("shell capture was not initialized")?;
        let sequence = self
            .entry_sequence
            .as_mut()
            .context("entry-sequence state is unavailable")?;
        ensure!(
            sequence.expected_next_tick == ENTRY_SEQUENCE_FRAMES,
            "entry sequence completed with {} accepted ticks",
            sequence.expected_next_tick
        );
        ensure!(
            sequence.pending.len() == usize::from(ENTRY_SEQUENCE_FRAMES),
            "entry sequence completed with {} retained readbacks",
            sequence.pending.len()
        );
        let generation = sequence
            .generation
            .context("entry sequence has no generation")?;
        sequence.completion_observed = true;
        let pending = std::mem::take(&mut sequence.pending);

        let mut payload = Vec::with_capacity(
            usize::try_from(ENTRY_SEQUENCE_BYTE_LENGTH)
                .context("entry-sequence payload length does not fit usize")?,
        );
        for (expected_tick, item) in pending.into_iter().enumerate() {
            ensure!(
                item.identity
                    == (EntrySequenceFrameIdentity {
                        generation,
                        tick: expected_tick as u8,
                    }),
                "entry-sequence retained readback identity changed"
            );
            let remaining = self
                .timeout()
                .checked_sub(started_at.elapsed())
                .context("entry sequence timeout expired during deferred readback")?;
            ensure!(
                !remaining.is_zero(),
                "entry sequence timeout expired during deferred readback"
            );
            let pixels =
                item.readback
                    .finish(&state.renderer.gpu.device, item.submission, remaining)?;
            ensure!(
                pixels.len() as u64 == FRAME_BYTE_LENGTH,
                "entry-sequence tick {expected_tick} length mismatch: expected \
                 {FRAME_BYTE_LENGTH}, got {}",
                pixels.len()
            );
            payload.extend_from_slice(&pixels);
        }
        ensure!(
            payload.len() as u64 == ENTRY_SEQUENCE_BYTE_LENGTH,
            "entry-sequence payload length mismatch: expected {ENTRY_SEQUENCE_BYTE_LENGTH}, got {}",
            payload.len()
        );

        fs::create_dir(self.request.output_dir()).with_context(|| {
            format!(
                "create immutable entry-sequence directory {}",
                self.request.output_dir().display()
            )
        })?;
        write_new_file(
            &self
                .request
                .output_dir()
                .join(ENTRY_SEQUENCE_FRAMES_FILE_NAME),
            &payload,
        )?;
        let manifest =
            entry_sequence_manifest(&self.request, state.renderer.gpu.config.format, generation);
        let mut manifest_bytes =
            serde_json::to_vec_pretty(&manifest).context("serialize entry-sequence manifest")?;
        manifest_bytes.push(b'\n');
        write_new_file(
            &self.request.output_dir().join(MANIFEST_FILE_NAME),
            &manifest_bytes,
        )?;
        self.outcome = Some(Ok(()));
        Ok(())
    }

    pub(crate) fn fail(&mut self, error: impl std::fmt::Display) {
        if self.outcome.is_none() {
            self.outcome = Some(Err(error.to_string()));
        }
    }

    pub(crate) fn take_outcome(&mut self) -> Result<()> {
        match self.outcome.take() {
            Some(Ok(())) => Ok(()),
            Some(Err(error)) => bail!("{error}"),
            None => bail!("shell capture event loop exited without a completed bundle"),
        }
    }
}

fn capture_manifest<'a>(
    request: &'a ShellCaptureRequest,
    surface_format: wgpu::TextureFormat,
    byte_length: u64,
) -> CaptureManifest<'a> {
    CaptureManifest {
        schema_version: CAPTURE_SCHEMA,
        checkpoint: request.checkpoint.as_str(),
        surface: SurfaceManifest {
            width: request.width,
            height: request.height,
            format: format!("{surface_format:?}"),
            pixel_layout: "BGRA8",
            row_order: "top-left",
            bytes_per_pixel: 4,
            row_stride: request.width * 4,
        },
        cursor: CursorManifest {
            x: request.cursor_x,
            y: request.cursor_y,
            policy: "software-composited",
        },
        shell: ShellManifest {
            screen: "main-menu",
            dialog_resource_id: 0x00E2,
            movie_owner: "main-menu-0xe2",
            movie_base: "ra2ts-l",
            main_menu_shell_failed: false,
            single_player_active: false,
            skirmish_active: false,
            modal_open: false,
            quit_active: false,
            first_paint_slide_active: false,
            title_terminal_persistent: true,
        },
        frame: FrameManifest {
            path: FRAME_FILE_NAME,
            byte_length,
        },
    }
}

fn entry_sequence_manifest(
    request: &ShellCaptureRequest,
    surface_format: wgpu::TextureFormat,
    generation: u64,
) -> EntrySequenceManifest {
    let frames = (0..ENTRY_SEQUENCE_FRAMES)
        .map(|tick| EntrySequenceFrameManifest {
            tick,
            byte_offset: u64::from(tick) * FRAME_BYTE_LENGTH,
            byte_length: FRAME_BYTE_LENGTH,
        })
        .collect();
    EntrySequenceManifest {
        schema_version: ENTRY_SEQUENCE_SCHEMA,
        checkpoint: request.checkpoint.as_str(),
        surface: SurfaceManifest {
            width: request.width,
            height: request.height,
            format: format!("{surface_format:?}"),
            pixel_layout: "BGRA8",
            row_order: "top-left",
            bytes_per_pixel: 4,
            row_stride: request.width * 4,
        },
        cursor: CursorManifest {
            x: request.cursor_x,
            y: request.cursor_y,
            policy: "software-composited",
        },
        shell: EntrySequenceShellManifest {
            screen: "main-menu",
            dialog_resource_id: 0x00E2,
            movie_owner: "main-menu-0xe2",
            movie_base: "ra2ts-l",
            title_hidden_during_frames: true,
        },
        presenter_domain: "final-swapchain-after-rgb565",
        generation,
        completion_observed: true,
        payload: EntrySequencePayloadManifest {
            path: ENTRY_SEQUENCE_FRAMES_FILE_NAME,
            byte_length: ENTRY_SEQUENCE_BYTE_LENGTH,
        },
        frames,
    }
}

/// One read-only startup projection for normal gameplay and the real loader.
/// Source membership remains the frozen launch; House IDs come only from the
/// constructed world supplied by its owner, never a previous resident runtime.
pub(crate) fn network_startup_receipt(
    state: &AppState,
    launch: &crate::skirmish_launch::SkirmishLaunchSession,
    sim: Option<&crate::sim::world::Simulation>,
) -> Result<serde_json::Value> {
    Ok(serde_json::json!({
        "match_id":crate::app::shell_wol::online_match_id(state),
        "seed":state.match_state.session.network_start_seed(),
        "map":launch.selected_map_file,
        "max_ahead":state.match_state.session.network_max_ahead_frames(),
        "local_source":launch.local_player_index,
        "sources":launch.humans.iter().enumerate().map(|(source,human)|serde_json::json!({
            "source":source,"name":human.name,"raw_row":human.raw_slot_index,"slot":human.slot,
            "house":sim.and_then(|sim|crate::app::match_runtime::network_match::source_house(
                &state.match_state.session,sim,source)),
            "endpoint":state.match_state.session.network_endpoint(source).map(|address|address.to_string())
        })).collect::<Vec<_>>()
    }))
}

fn write_new_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("create new capture artifact {}", path.display()))?;
    file.write_all(bytes)
        .with_context(|| format!("write capture artifact {}", path.display()))?;
    file.flush()
        .with_context(|| format!("flush capture artifact {}", path.display()))?;
    Ok(())
}

#[derive(Serialize)]
struct CaptureManifest<'a> {
    schema_version: &'a str,
    checkpoint: &'a str,
    surface: SurfaceManifest,
    cursor: CursorManifest,
    shell: ShellManifest,
    frame: FrameManifest<'a>,
}

#[derive(Serialize)]
struct SurfaceManifest {
    width: u32,
    height: u32,
    format: String,
    pixel_layout: &'static str,
    row_order: &'static str,
    bytes_per_pixel: u32,
    row_stride: u32,
}

#[derive(Serialize)]
struct CursorManifest {
    x: u32,
    y: u32,
    policy: &'static str,
}

#[derive(Serialize)]
struct ShellManifest {
    screen: &'static str,
    dialog_resource_id: u32,
    movie_owner: &'static str,
    movie_base: &'static str,
    main_menu_shell_failed: bool,
    single_player_active: bool,
    skirmish_active: bool,
    modal_open: bool,
    quit_active: bool,
    first_paint_slide_active: bool,
    title_terminal_persistent: bool,
}

#[derive(Serialize)]
struct FrameManifest<'a> {
    path: &'a str,
    byte_length: u64,
}

#[derive(Serialize)]
struct EntrySequenceManifest {
    schema_version: &'static str,
    checkpoint: &'static str,
    surface: SurfaceManifest,
    cursor: CursorManifest,
    shell: EntrySequenceShellManifest,
    presenter_domain: &'static str,
    generation: u64,
    completion_observed: bool,
    payload: EntrySequencePayloadManifest,
    frames: Vec<EntrySequenceFrameManifest>,
}

#[derive(Serialize)]
struct EntrySequenceShellManifest {
    screen: &'static str,
    dialog_resource_id: u32,
    movie_owner: &'static str,
    movie_base: &'static str,
    title_hidden_during_frames: bool,
}

#[derive(Serialize)]
struct EntrySequencePayloadManifest {
    path: &'static str,
    byte_length: u64,
}

#[derive(Serialize)]
struct EntrySequenceFrameManifest {
    tick: u8,
    byte_offset: u64,
    byte_length: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    fn new_output_path(tag: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "vera20k-shell-capture-{}-{tag}-{unique}",
            std::process::id()
        ))
    }

    fn valid_args(output: &Path) -> Vec<OsString> {
        args(&[
            "--shell-capture",
            "main-menu-0xe2-steady",
            "--width",
            "800",
            "--height",
            "600",
            "--cursor-x",
            "400",
            "--cursor-y",
            "300",
            "--output",
            output.to_str().expect("UTF-8 temp path"),
        ])
    }

    #[test]
    fn no_args_preserves_interactive_launch() {
        assert!(matches!(
            parse_launch_args(Vec::<OsString>::new()).expect("parse"),
            AppLaunchMode::Interactive
        ));
    }

    #[test]
    fn strict_capture_args_build_expected_request() {
        let output = new_output_path("valid");
        let launch = parse_launch_args(valid_args(&output)).expect("parse");
        let AppLaunchMode::ShellCapture(request) = launch else {
            panic!("expected capture");
        };
        assert_eq!(
            request.checkpoint(),
            ShellCaptureCheckpoint::MainMenu0xE2Steady
        );
        assert_eq!((request.width(), request.height()), (800, 600));
        assert_eq!((request.cursor_x(), request.cursor_y()), (400, 300));
        assert_eq!(request.output_dir(), output);
    }

    #[test]
    fn live_lan_capture_requires_two_explicit_unicast_endpoints() {
        let output = new_output_path("live-lan");
        let mut values = valid_args(&output);
        values[1] = "lan-live-host".into();
        assert!(parse_launch_args(values.clone()).is_err());
        values.extend(args(&[
            "--lan-bind",
            "127.0.0.1:31000",
            "--lan-peer",
            "127.0.0.1:31010",
        ]));
        let AppLaunchMode::ShellCapture(request) = parse_launch_args(values.clone()).unwrap()
        else {
            panic!("capture");
        };
        assert_eq!(
            request.live_lan,
            Some(LiveLanConfig {
                transport: LiveLanTransport::Explicit {
                    bind: "127.0.0.1:31000".parse().unwrap(),
                    peer: "127.0.0.1:31010".parse().unwrap(),
                },
                map: None,
                setup_crash: None,
                profile_check: false,
                failure_prefix_output: None,
            })
        );
        values[1] = "lan-live-guest".into();
        assert!(parse_launch_args(values.clone()).is_ok());
        values[1] = "lan-battle-host".into();
        assert!(parse_launch_args(values.clone()).is_ok());
        values[1] = "lan-battle-guest".into();
        assert!(parse_launch_args(values.clone()).is_ok());
        values[1] = "lan-0xbb-steady".into();
        assert!(parse_launch_args(values).is_err());
    }

    #[test]
    fn production_lan_broadcast_needs_no_socket_overrides() {
        let output = new_output_path("lan-production-broadcast");
        let mut values = valid_args(&output);
        values[1] = "lan-live-host".into();
        values.extend(args(&[
            "--lan-production-broadcast",
            "--lan-map",
            "host-only.map",
        ]));
        let AppLaunchMode::ShellCapture(request) = parse_launch_args(values.clone()).unwrap()
        else {
            panic!("capture");
        };
        let config = request.live_lan.unwrap();
        assert_eq!(config.transport, LiveLanTransport::ProductionBroadcast);
        assert_eq!(config.map.as_deref(), Some("host-only.map"));
        values.truncate(values.len() - 2);
        values[1] = "lan-live-guest".into();
        let AppLaunchMode::ShellCapture(request) = parse_launch_args(values).unwrap() else {
            panic!("capture");
        };
        assert_eq!(
            request.live_lan.unwrap().transport,
            LiveLanTransport::ProductionBroadcast
        );
    }

    #[test]
    fn production_lan_broadcast_rejects_socket_or_fault_overrides() {
        let output = new_output_path("lan-production-conflict");
        let mut values = valid_args(&output);
        values[1] = "lan-live-host".into();
        values.push("--lan-production-broadcast".into());
        for extra in [
            args(&["--lan-bind", "127.0.0.1:31000"]),
            args(&["--lan-peer", "127.0.0.1:31010"]),
            args(&["--lan-setup-crash", "guest"]),
        ] {
            let mut invalid = values.clone();
            invalid.extend(extra);
            assert!(
                parse_launch_args(invalid)
                    .unwrap_err()
                    .to_string()
                    .contains("cannot override sockets or combine with a fault")
            );
        }
        values.push("--lan-production-broadcast".into());
        assert!(
            parse_launch_args(values)
                .unwrap_err()
                .to_string()
                .contains("duplicate --lan-production-broadcast")
        );
    }

    #[test]
    fn production_lan_broadcast_requires_normal_full_round_targets() {
        let output = new_output_path("lan-production-target");
        for target in [
            "lan-battle-host",
            "lan-live-timeout-host",
            "lan-live-desync-guest",
            "online-live-host",
            "lan-0xbb-steady",
        ] {
            let mut values = valid_args(&output);
            values[1] = target.into();
            values.push("--lan-production-broadcast".into());
            assert!(
                parse_launch_args(values)
                    .unwrap_err()
                    .to_string()
                    .contains("requires a normal LAN live host or guest"),
                "{target}"
            );
        }
    }

    #[test]
    fn live_lan_host_map_uses_a_logical_name_for_custom_and_generated_rounds() {
        let output = new_output_path("live-lan-map");
        let mut values = valid_args(&output);
        values[1] = "lan-live-host".into();
        values.extend(args(&[
            "--lan-bind",
            "127.0.0.1:31000",
            "--lan-peer",
            "127.0.0.1:31010",
            "--lan-map",
            "host-only.map",
        ]));
        let AppLaunchMode::ShellCapture(request) = parse_launch_args(values.clone()).unwrap()
        else {
            panic!("capture");
        };
        assert_eq!(
            request.live_lan.unwrap().map.as_deref(),
            Some("host-only.map")
        );
        *values.last_mut().unwrap() = "RandMap.Sed".into();
        let AppLaunchMode::ShellCapture(request) = parse_launch_args(values).unwrap() else {
            panic!("capture");
        };
        assert_eq!(
            request.live_lan.unwrap().map.as_deref(),
            Some("RandMap.Sed")
        );
    }

    #[test]
    fn live_lan_map_rejects_guest_targets_and_remote_host_paths() {
        let output = new_output_path("live-lan-map-invalid");
        let mut values = valid_args(&output);
        values[1] = "lan-live-host".into();
        values.extend(args(&[
            "--lan-bind",
            "127.0.0.1:31000",
            "--lan-peer",
            "127.0.0.1:31010",
            "--lan-map",
            "../outside.map",
        ]));
        assert!(parse_launch_args(values.clone()).is_err());
        *values.last_mut().unwrap() = "host-only.map".into();
        values[1] = "lan-live-guest".into();
        assert!(parse_launch_args(values.clone()).is_err());
        values[1] = "lan-battle-host".into();
        assert!(parse_launch_args(values).is_err());
    }

    #[test]
    fn lan_desync_pair_writes_real_prefixes_without_online_credentials() {
        let output = new_output_path("lan-desync-prefix");
        let mut values = valid_args(&output);
        values[1] = "lan-live-desync-host".into();
        values.extend(args(&[
            "--lan-bind",
            "127.0.0.1:31000",
            "--lan-peer",
            "127.0.0.1:31050",
        ]));
        let AppLaunchMode::ShellCapture(request) = parse_launch_args(values.clone()).unwrap()
        else {
            panic!("capture");
        };
        assert_eq!(
            request.live_lan.unwrap().failure_prefix_output.unwrap(),
            PathBuf::from(format!("{}.prefix.json", output.display()))
        );
        values[1] = "lan-live-desync-guest".into();
        assert!(parse_launch_args(values).is_ok());
    }

    #[test]
    fn editable_lan_crash_checkpoint_requires_a_real_pair_and_owned_prefix() {
        let output = new_output_path("lan-setup-crash");
        let mut values = valid_args(&output);
        values[1] = "lan-live-host".into();
        values.extend(args(&[
            "--lan-bind",
            "127.0.0.1:31000",
            "--lan-peer",
            "127.0.0.1:31010",
            "--lan-setup-crash",
            "guest",
        ]));
        let AppLaunchMode::ShellCapture(request) = parse_launch_args(values.clone()).unwrap()
        else {
            panic!("capture")
        };
        let lan = request.live_lan.unwrap();
        assert_eq!(lan.setup_crash, Some(LanSetupCrash::Guest));
        assert_eq!(
            lan.failure_prefix_output,
            Some(PathBuf::from(format!("{}.prefix.json", output.display())))
        );
        values[1] = "lan-live-guest".into();
        *values.last_mut().unwrap() = "host".into();
        assert!(parse_launch_args(values.clone()).is_ok());
        values[1] = "lan-battle-guest".into();
        assert!(parse_launch_args(values.clone()).is_err());
        values[1] = "lan-live-guest".into();
        *values.last_mut().unwrap() = "timeout".into();
        assert!(parse_launch_args(values).is_err());
    }

    #[test]
    fn interrupted_map_host_checkpoint_requires_authored_host_selection() {
        let output = new_output_path("lan-map-crash");
        let mut values = valid_args(&output);
        values[1] = "lan-live-host".into();
        values.extend(args(&[
            "--lan-bind",
            "127.0.0.1:31000",
            "--lan-peer",
            "127.0.0.1:31010",
            "--lan-setup-crash",
            "map-host",
        ]));
        assert!(parse_launch_args(values.clone()).is_err());
        values[1] = "lan-live-guest".into();
        assert!(parse_launch_args(values.clone()).is_ok());
        values[1] = "lan-live-host".into();
        values.extend(args(&["--lan-map", "transfer.yrm"]));
        let AppLaunchMode::ShellCapture(request) = parse_launch_args(values.clone()).unwrap()
        else {
            panic!("capture");
        };
        assert_eq!(
            request.live_lan.unwrap().setup_crash,
            Some(LanSetupCrash::MapHost)
        );
        *values.last_mut().unwrap() = "RandMap.Sed".into();
        assert!(parse_launch_args(values).is_err());
    }

    #[test]
    fn live_lan_capture_rejects_overlapping_and_invalid_socket_parameters() {
        let output = new_output_path("live-lan-invalid");
        for (bind, peer) in [
            ("127.0.0.1:65534", "127.0.0.1:31010"),
            ("127.0.0.1:31000", "127.0.0.1:31002"),
            ("127.0.0.1:31000", "0.0.0.0:31010"),
        ] {
            let mut values = valid_args(&output);
            values[1] = "lan-live-host".into();
            values.extend(args(&["--lan-bind", bind, "--lan-peer", peer]));
            assert!(parse_launch_args(values).is_err(), "{bind}/{peer}");
        }
    }

    #[test]
    fn online_guest_capture_requires_a_username_and_an_explicit_host() {
        let output = new_output_path("online-guest");
        let mut values = valid_args(&output);
        values[1] = "online-live-guest".into();
        assert!(parse_launch_args(values.clone()).is_err());
        values.extend(args(&["--online-account", "Peer"]));
        assert!(parse_launch_args(values.clone()).is_err());
        values.extend(args(&["--online-host", "Host"]));
        let AppLaunchMode::ShellCapture(request) = parse_launch_args(values).unwrap() else {
            panic!("capture")
        };
        let config = request.live_online.unwrap();
        assert_eq!(config.account, "Peer");
        assert_eq!(config.host, "Host");
        assert_eq!(config.players, 2);
    }

    #[test]
    fn online_capture_rejects_password_and_registration_options() {
        let output = new_output_path("online-username-only");
        let mut values = valid_args(&output);
        values[1] = "online-live-host".into();
        values.extend(args(&["--online-account", "Host"]));
        assert!(parse_launch_args(values.clone()).is_ok());
        values.push("--online-password-stdin".into());
        assert!(parse_launch_args(values.clone()).is_err());
        *values.last_mut().unwrap() = "--online-create-account".into();
        assert!(parse_launch_args(values).is_err());
    }

    #[test]
    fn online_ai_host_capture_keeps_one_human_and_the_requested_retail_mode() {
        let output = new_output_path("online-ai-host");
        let mut values = valid_args(&output);
        values[1] = "online-live-host".into();
        values.extend(args(&[
            "--online-account",
            "Host",
            "--online-players",
            "1",
            "--online-ai",
            "hard",
            "--online-mode-id",
            "1",
        ]));
        let AppLaunchMode::ShellCapture(request) = parse_launch_args(values).unwrap() else {
            panic!("capture")
        };
        let config = request.live_online.unwrap();
        assert_eq!(config.players, 1);
        assert_eq!(config.ai, Some(crate::skirmish_launch::AiDifficulty::Hard));
        assert_eq!(config.mode_id, Some(1));
        assert_eq!(config.frames, 600);
        assert!(config.simple_input && !config.mass_input);
    }

    #[test]
    fn online_latency_is_opt_in_and_rejects_conflicting_modes() {
        let output = new_output_path("online-latency");
        let mut values = valid_args(&output);
        values[1] = "online-live-host".into();
        values.extend(args(&["--online-account", "Host", "--online-latency"]));
        let AppLaunchMode::ShellCapture(request) = parse_launch_args(values.clone()).unwrap()
        else {
            panic!("capture");
        };
        assert_eq!((request.width(), request.height()), (800, 600));
        let config = request.live_online.unwrap();
        assert!(config.latency && config.simple_input);
        assert_eq!(config.frames, 6000);
        let mut small = values.clone();
        let width = small.iter().position(|arg| arg == "--width").unwrap();
        let height = small.iter().position(|arg| arg == "--height").unwrap();
        small[width + 1] = "640".into();
        small[height + 1] = "480".into();
        small.extend(args(&["--online-frames", "9000"]));
        let AppLaunchMode::ShellCapture(request) = parse_launch_args(small.clone()).unwrap() else {
            panic!("capture")
        };
        assert_eq!((request.width(), request.height()), (640, 480));
        assert_eq!(request.live_online.unwrap().frames, 9000);
        small.retain(|arg| arg != "--online-latency");
        assert!(
            parse_launch_args(small).is_err(),
            "ordinary captures retain800x600"
        );
        values.extend(args(&["--online-mass-input"]));
        assert!(parse_launch_args(values).is_err());
    }

    #[test]
    fn online_ai_capture_requires_a_singleton_host_and_actual_play() {
        let output = new_output_path("online-ai-admission");
        let mut values = valid_args(&output);
        values[1] = "online-live-host".into();
        values.extend(args(&["--online-account", "Host", "--online-players", "1"]));
        assert!(parse_launch_args(values.clone()).is_err());
        values.extend(args(&["--online-ai", "normal"]));
        assert!(parse_launch_args(values.clone()).is_ok());
        let mut short = values.clone();
        short.extend(args(&["--online-frames", "300"]));
        assert!(parse_launch_args(short).is_err());
        values[1] = "online-live-guest".into();
        values.extend(args(&["--online-host", "Host"]));
        assert!(parse_launch_args(values).is_err());
    }

    #[test]
    fn online_ai_options_reject_unknown_difficulties_and_nononline_routes() {
        let output = new_output_path("online-ai-invalid");
        let mut values = valid_args(&output);
        values[1] = "online-live-host".into();
        values.extend(args(&[
            "--online-account",
            "Host",
            "--online-players",
            "1",
            "--online-ai",
            "unknown",
        ]));
        assert!(parse_launch_args(values.clone()).is_err());
        *values.last_mut().unwrap() = "easy".into();
        assert!(parse_launch_args(values.clone()).is_ok());
        values[1] = "main-menu-0xe2-steady".into();
        assert!(parse_launch_args(values).is_err());
    }

    #[test]
    fn online_failure_targets_require_a_pair_and_write_a_separate_real_prefix() {
        for target in [
            "online-live-timeout-host",
            "online-live-quit-host",
            "online-live-failure-guest",
        ] {
            let output = new_output_path("online-failure");
            let mut values = valid_args(&output);
            values[1] = target.into();
            values.extend(args(&["--online-account", "Peer", "--online-host", "Host"]));
            let AppLaunchMode::ShellCapture(request) = parse_launch_args(values.clone()).unwrap()
            else {
                panic!("capture")
            };
            let config = request.live_online.unwrap();
            assert_eq!(
                (
                    config.players,
                    config.frames,
                    config.simple_input,
                    config.mass_input
                ),
                (2, 300, true, false)
            );
            let mut prefix = output.as_os_str().to_owned();
            prefix.push(".prefix.json");
            assert_eq!(config.failure_prefix_output, Some(PathBuf::from(prefix)));
            values.extend(args(&["--online-players", "3"]));
            assert!(parse_launch_args(values).is_err());
        }
        let output = new_output_path("online-failure-host-required");
        let mut values = valid_args(&output);
        values[1] = "online-live-failure-guest".into();
        values.extend(args(&["--online-account", "Peer"]));
        assert!(parse_launch_args(values).is_err());
    }

    #[test]
    fn network_checkpoint_requires_a_normal_pair_and_a_bounded_public_nonce() {
        let output = new_output_path("online-network-checkpoint");
        let mut values = valid_args(&output);
        values[1] = "online-live-host".into();
        values.extend(args(&[
            "--online-account",
            "Host",
            "--online-simple-input",
            "--online-frames",
            "600",
            "--online-network-checkpoint",
            "0123456789abcdef0123456789abcdef",
        ]));
        let AppLaunchMode::ShellCapture(request) = parse_launch_args(values.clone()).unwrap()
        else {
            panic!("capture");
        };
        assert_eq!(
            request.live_online.unwrap().network_checkpoint.as_deref(),
            Some("0123456789abcdef0123456789abcdef")
        );
        let frames = values.iter().position(|value| value == "600").unwrap();
        values[frames] = "300".into();
        assert!(parse_launch_args(values.clone()).is_err());
        values[frames] = "600".into();
        values[1] = "online-live-timeout-host".into();
        assert!(parse_launch_args(values).is_err());
    }

    #[test]
    fn loading_cancellation_and_progress_are_explicit_normal_online_inputs() {
        let output = new_output_path("online-loading-cancel");
        let mut values = valid_args(&output);
        values[1] = "online-live-host".into();
        values.extend(args(&[
            "--online-account",
            "CaptureHost",
            "--online-simple-input",
            "--online-loading-cancel",
            "https",
            "--online-loading-cancel-checkpoint",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "--online-observe-loading",
        ]));
        let AppLaunchMode::ShellCapture(request) = parse_launch_args(values.clone()).unwrap()
        else {
            panic!("capture")
        };
        let config = request.live_online.unwrap();
        assert!(config.observe_loading);
        assert_eq!(
            config.loading_cancel.unwrap().kind,
            LoadingCancelKind::Https
        );
        values.extend(args(&["--online-players", "3"]));
        assert!(parse_launch_args(values).is_err());
        let mut wrong = valid_args(&output);
        wrong[1] = "online-live-host".into();
        wrong.extend(args(&[
            "--online-account",
            "CaptureHost",
            "--online-simple-input",
            "--online-loading-cancel",
            "departure",
        ]));
        assert!(parse_launch_args(wrong).is_err());
    }

    #[test]
    fn loading_host_loss_requires_the_normal_three_player_input_route() {
        let output = new_output_path("online-loading-host-loss");
        let mut values = valid_args(&output);
        values[1] = "online-live-guest".into();
        values.extend(args(&[
            "--online-account",
            "Guest",
            "--online-host",
            "Host",
            "--online-simple-input",
            "--online-players",
            "3",
            "--online-frames",
            "1024",
            "--online-loading-host-loss",
            "0123456789abcdef0123456789abcdef",
        ]));
        let AppLaunchMode::ShellCapture(request) = parse_launch_args(values.clone()).unwrap()
        else {
            panic!("capture");
        };
        let config = request.live_online.unwrap();
        assert_eq!(config.players, 3);
        assert_eq!(config.loading_output, Some(output));
        assert_eq!(
            config.loading_host_loss.as_deref(),
            Some("0123456789abcdef0123456789abcdef")
        );
        let count = values.iter().position(|value| value == "3").unwrap();
        values[count] = "2".into();
        assert!(parse_launch_args(values.clone()).is_err());
        values[count] = "3".into();
        values.extend(args(&["--online-map", "custom.map"]));
        assert!(parse_launch_args(values).is_err());
    }

    #[test]
    fn exit_reentry_checkpoint_uses_a_separate_public_nonce_and_normal_pair() {
        let output = new_output_path("online-exit-checkpoint");
        let mut values = valid_args(&output);
        values[1] = "online-live-guest".into();
        values.extend(args(&[
            "--online-account",
            "Guest",
            "--online-host",
            "Host",
            "--online-simple-input",
            "--online-exit-checkpoint",
            "0123456789abcdef0123456789abcdef",
        ]));
        let AppLaunchMode::ShellCapture(request) = parse_launch_args(values.clone()).unwrap()
        else {
            panic!("capture");
        };
        let config = request.live_online.unwrap();
        assert!(config.failure_prefix_output.is_none());
        assert!(
            config
                .exit_prefix_output
                .unwrap()
                .to_string_lossy()
                .ends_with(".exit.json")
        );
        assert_eq!(
            config.exit_checkpoint.as_deref(),
            Some("0123456789abcdef0123456789abcdef")
        );
        *values.last_mut().unwrap() = "short".into();
        assert!(parse_launch_args(values.clone()).is_err());
        *values.last_mut().unwrap() = "0123456789abcdef0123456789abcdef".into();
        values[1] = "online-live-timeout-host".into();
        assert!(parse_launch_args(values).is_err());
    }

    #[test]
    fn scale_capture_uses_explicit_player_and_observation_limits() {
        let output = new_output_path("online-scale");
        let mut values = valid_args(&output);
        values[1] = "online-live-host".into();
        values.extend(args(&[
            "--online-account",
            "Host",
            "--online-players",
            "30",
            "--online-frames",
            "900",
            "--online-mass-input",
            "--online-map",
            "scale.map",
            "--online-start",
            "29",
        ]));
        let AppLaunchMode::ShellCapture(request) = parse_launch_args(values.clone()).unwrap()
        else {
            panic!("capture")
        };
        let config = request.live_online.unwrap();
        assert_eq!((config.players, config.frames), (30, 900));
        assert!(config.simple_input && config.mass_input);
        assert_eq!(config.map.as_deref(), Some("scale.map"));
        assert_eq!(config.start, Some(29));
        let mut invalid_start = values.clone();
        let start = invalid_start
            .iter()
            .position(|value| value == "--online-start")
            .unwrap();
        invalid_start[start + 1] = "30".into();
        assert!(parse_launch_args(invalid_start).is_err());
        let mut generated = values.clone();
        let map = generated
            .iter()
            .position(|value| value == "scale.map")
            .unwrap();
        generated[map] = "RandMap.Sed".into();
        assert!(
            parse_launch_args(generated).is_err(),
            "generated maps retain their eight-start limit"
        );
        let frames = values.iter().position(|value| value == "900").unwrap();
        values[frames] = "100".into();
        assert!(parse_launch_args(values).is_err());
    }

    #[test]
    fn strict_capture_args_accept_main_menu_entry_sequence() {
        let output = new_output_path("entry-sequence");
        let mut values = valid_args(&output);
        values[1] = OsString::from("main-menu-0xe2-entry-sequence");
        let AppLaunchMode::ShellCapture(request) =
            parse_launch_args(values).expect("entry sequence checkpoint must parse")
        else {
            panic!("expected capture");
        };
        assert_eq!(
            request.checkpoint().as_str(),
            "main-menu-0xe2-entry-sequence"
        );
    }

    #[test]
    fn slide_checkpoints_accept_only_shown_ticks() {
        // Every family slide at 800x600 shows ticks 0..=17 (9 tile rows + 9).
        for (prefix, checkpoint) in [
            (
                "main-menu-0xe2-slide-out-tick-",
                ShellCaptureCheckpoint::MainMenu0xE2SlideOut(17),
            ),
            (
                "movie-list-0x129-slide-out-tick-",
                ShellCaptureCheckpoint::MovieList0x129SlideOut(17),
            ),
            (
                "skirmish-0x102-back-slide-out-tick-",
                ShellCaptureCheckpoint::Skirmish0x102BackSlideOut(17),
            ),
            (
                "skirmish-0x102-entry-tick-",
                ShellCaptureCheckpoint::Skirmish0x102Entry(17),
            ),
            (
                "skirmish-0x105-entry-tick-",
                ShellCaptureCheckpoint::Skirmish0x105Entry(17),
            ),
            (
                "skirmish-0x105-cancel-tick-",
                ShellCaptureCheckpoint::Skirmish0x105CancelEntry(17),
            ),
            (
                "campaign-0x94-entry-tick-",
                ShellCaptureCheckpoint::Campaign0x94Entry(17),
            ),
            (
                "load-saved-game-0xb7-entry-tick-",
                ShellCaptureCheckpoint::LoadSavedGame0xB7Entry(17),
            ),
            (
                "options-0xd5-entry-tick-",
                ShellCaptureCheckpoint::Options0xD5Entry(17),
            ),
        ] {
            assert_eq!(
                ShellCaptureCheckpoint::parse(&format!("{prefix}17")).expect("last tick"),
                checkpoint
            );
            assert!(ShellCaptureCheckpoint::parse(&format!("{prefix}18")).is_err());
        }
    }

    #[test]
    fn options_hover_checkpoints_round_trip_their_names() {
        for (name, _) in OPTIONS_0XD5_HOVER_CHECKPOINTS
            .into_iter()
            .chain(SKIRMISH_0X102_HOVER_CHECKPOINTS)
            .chain(SKIRMISH_0X105_HOVER_CHECKPOINTS)
        {
            let checkpoint = ShellCaptureCheckpoint::parse(name).expect("hover checkpoint");
            assert_eq!(checkpoint.as_str(), name);
        }
        assert!(ShellCaptureCheckpoint::parse("options-0xd5-hover-nothing").is_err());
        let vera =
            ShellCaptureCheckpoint::parse(CHECKPOINT_OPTIONS_VERA_STEADY).expect("VERA page");
        assert_eq!(vera, ShellCaptureCheckpoint::OptionsVeraSteady);
        assert_eq!(vera.as_str(), CHECKPOINT_OPTIONS_VERA_STEADY);
    }

    #[test]
    fn lan_checkpoints_route_through_production_network() {
        for &(name, target) in movies::LAN_CHECKPOINTS {
            let checkpoint = ShellCaptureCheckpoint::parse(name).expect("LAN checkpoint");
            assert_eq!(checkpoint.as_str(), name);
            assert_eq!(
                checkpoint.movies_target(),
                Some(movies::MoviesTarget::Lan(target))
            );
        }
        assert!(ShellCaptureCheckpoint::parse("lan-0xbd-offline-shortcut").is_err());
    }

    #[test]
    fn wol_checkpoints_route_through_production_wol() {
        for &(name, target) in movies::WOL_CHECKPOINTS {
            let checkpoint = ShellCaptureCheckpoint::parse(name).expect("WOL checkpoint");
            assert_eq!(checkpoint.as_str(), name);
            assert_eq!(
                checkpoint.movies_target(),
                Some(movies::MoviesTarget::Wol(target))
            );
        }
    }

    #[test]
    fn network_options_checkpoints_route_through_production_options() {
        for (name, target) in NETWORK_OPTIONS_CHECKPOINTS {
            let checkpoint = ShellCaptureCheckpoint::parse(name).expect("D7 checkpoint");
            assert_eq!(checkpoint.as_str(), name);
            assert_eq!(
                checkpoint.movies_target(),
                Some(movies::MoviesTarget::NetworkOptions(target))
            );
        }
    }

    #[test]
    fn wol_checkpoints_round_trip() {
        let AppLaunchMode::ShellCapture(mut request) =
            parse_launch_args(valid_args(&new_output_path("legacy-wol"))).unwrap()
        else {
            panic!("capture");
        };
        for name in [
            CHECKPOINT_WOL_0X10E_STEADY,
            CHECKPOINT_WOL_0X10E_HOVER_QUICK_MATCH,
            CHECKPOINT_WOL_0X10E_API_MISSING,
            CHECKPOINT_WOL_0X10E_BACK,
            "wol-0x10e-entry-tick-0",
        ] {
            let checkpoint = ShellCaptureCheckpoint::parse(name).expect("WOL checkpoint");
            request.checkpoint = checkpoint;
            assert!(request.native_shell_ui());
            assert_eq!(
                request.wol_welcome_provider(),
                Some(crate::app::shell_wol::WolProvider::Unavailable)
            );
            if !name.contains("entry-tick") {
                assert_eq!(checkpoint.as_str(), name);
            }
        }
        for (name, provider) in [
            (
                "wol-0x113-login-accepted",
                crate::app::shell_wol::WolProvider::Controlled,
            ),
            (
                "wol-0x10e-profile",
                crate::app::shell_wol::WolProvider::Controlled,
            ),
            (
                "wol-test-service-lobby",
                crate::app::shell_wol::WolProvider::TestService,
            ),
        ] {
            request.checkpoint = ShellCaptureCheckpoint::parse(name).unwrap();
            assert!(request.native_shell_ui());
            assert_eq!(request.wol_welcome_provider(), Some(provider));
        }
        assert_eq!(
            ShellCaptureCheckpoint::parse("wol-0x10e-entry-tick-17").expect("last tick"),
            ShellCaptureCheckpoint::Wol0x10EEntry(17)
        );
        assert!(ShellCaptureCheckpoint::parse("wol-0x10e-entry-tick-18").is_err());
    }

    #[test]
    fn modern_wol_captures_require_no_login_inputs() {
        let output = new_output_path("modern-wol");
        let mut values = valid_args(&output);
        values[1] = "wol-modern-username".into();
        let AppLaunchMode::ShellCapture(mut request) = parse_launch_args(values).expect("parse")
        else {
            panic!("capture");
        };
        assert!(request.live_online.is_none());
        request.checkpoint = ShellCaptureCheckpoint::Lan(movies::LanTarget::OnlineHost);
        assert!(!request.native_shell_ui());
        assert_eq!(request.wol_welcome_provider(), None);
        request.checkpoint = ShellCaptureCheckpoint::Lan(movies::LanTarget::Accepted);
        assert!(request.native_shell_ui(), "BD diagnostics keep Accept");
        request.checkpoint = ShellCaptureCheckpoint::parse("wol-modern-lobby").unwrap();
        assert!(!request.native_shell_ui());
        assert!(request.live_online.is_none());
        assert_eq!(
            request.wol_welcome_provider(),
            Some(crate::app::shell_wol::WolProvider::TestService)
        );
        request.checkpoint = ShellCaptureCheckpoint::parse("wol-modern-create").unwrap();
        assert!(!request.native_shell_ui());
        request.checkpoint = ShellCaptureCheckpoint::parse("wol-modern-username").unwrap();
        assert!(!request.native_shell_ui());
        assert_eq!(
            request.wol_welcome_provider(),
            Some(crate::app::shell_wol::WolProvider::Online)
        );
    }

    #[test]
    fn campaign_start_checkpoints_select_only_the_normal_route_targets() {
        use crate::ui::campaign_shell::CampaignSide::{Allied, Soviet};
        for (name, target) in [
            (
                "campaign-all1-initial-save",
                movies::MoviesTarget::CampaignStart {
                    side: Allied,
                    target: movies::CampaignStartTarget::InitialSaved,
                },
            ),
            (
                "campaign-sov1-initial-save",
                movies::MoviesTarget::CampaignStart {
                    side: Soviet,
                    target: movies::CampaignStartTarget::InitialSaved,
                },
            ),
            (
                "campaign-all02-first-live-frame",
                movies::MoviesTarget::CampaignStart {
                    side: Allied,
                    target: movies::CampaignStartTarget::ExplicitFirstLive("ALL02UMD.MAP"),
                },
            ),
            (
                "campaign-sov05-first-live-frame",
                movies::MoviesTarget::CampaignStart {
                    side: Soviet,
                    target: movies::CampaignStartTarget::ExplicitFirstLive("SOV05UMD.MAP"),
                },
            ),
            (
                "campaign-sov07-first-live-frame",
                movies::MoviesTarget::CampaignStart {
                    side: Soviet,
                    target: movies::CampaignStartTarget::ExplicitFirstLive("SOV07TMD.MAP"),
                },
            ),
            (
                CHECKPOINT_CAMPAIGN_ALL1_LOADING_FIRST,
                movies::MoviesTarget::CampaignStart {
                    side: Allied,
                    target: movies::CampaignStartTarget::LoadingFirst,
                },
            ),
            (
                CHECKPOINT_CAMPAIGN_SOV1_LOADING_FIRST,
                movies::MoviesTarget::CampaignStart {
                    side: Soviet,
                    target: movies::CampaignStartTarget::LoadingFirst,
                },
            ),
            (
                CHECKPOINT_CAMPAIGN_ALL1_FIRST_LIVE,
                movies::MoviesTarget::CampaignStart {
                    side: Allied,
                    target: movies::CampaignStartTarget::FirstLive,
                },
            ),
            (
                CHECKPOINT_CAMPAIGN_SOV1_FIRST_LIVE,
                movies::MoviesTarget::CampaignStart {
                    side: Soviet,
                    target: movies::CampaignStartTarget::FirstLive,
                },
            ),
            (
                "campaign-sov02-objective-expiry",
                movies::MoviesTarget::CampaignStart {
                    side: Soviet,
                    target: movies::CampaignStartTarget::Restart(
                        movies::RestartTarget::ObjectiveExpiry,
                    ),
                },
            ),
            (
                "campaign-sov6-alternate-home-victory",
                movies::MoviesTarget::CampaignStart {
                    side: Soviet,
                    target: movies::CampaignStartTarget::Restart(
                        movies::RestartTarget::AlternateHomeVictory,
                    ),
                },
            ),
            (
                CHECKPOINT_CAMPAIGN_ALL1_ABORT_RETURN,
                movies::MoviesTarget::CampaignStart {
                    side: Allied,
                    target: movies::CampaignStartTarget::AbortReturn,
                },
            ),
            (
                CHECKPOINT_CAMPAIGN_SOV1_ABORT_RETURN,
                movies::MoviesTarget::CampaignStart {
                    side: Soviet,
                    target: movies::CampaignStartTarget::AbortReturn,
                },
            ),
            (
                CHECKPOINT_CAMPAIGN_ALL1_ABORT_RESTART_SHOWN,
                movies::MoviesTarget::CampaignStart {
                    side: Allied,
                    target: movies::CampaignStartTarget::Restart(movies::RestartTarget::AbortShown),
                },
            ),
            (
                CHECKPOINT_CAMPAIGN_ALL1_ABORT_RESTART,
                movies::MoviesTarget::CampaignStart {
                    side: Allied,
                    target: movies::CampaignStartTarget::Restart(
                        movies::RestartTarget::AbortRestarted,
                    ),
                },
            ),
            (
                CHECKPOINT_CAMPAIGN_ALL1_DEFEAT_REPLAY_PROMPT,
                movies::MoviesTarget::CampaignStart {
                    side: Allied,
                    target: movies::CampaignStartTarget::Restart(
                        movies::RestartTarget::DefeatPrompt,
                    ),
                },
            ),
            (
                CHECKPOINT_CAMPAIGN_ALL1_DEFEAT_REPLAY_NO,
                movies::MoviesTarget::CampaignStart {
                    side: Allied,
                    target: movies::CampaignStartTarget::Restart(
                        movies::RestartTarget::DefeatReturned,
                    ),
                },
            ),
            (
                CHECKPOINT_CAMPAIGN_ALL1_DEFEAT_REPLAY_YES,
                movies::MoviesTarget::CampaignStart {
                    side: Allied,
                    target: movies::CampaignStartTarget::Restart(
                        movies::RestartTarget::DefeatReplayed,
                    ),
                },
            ),
            (
                "campaign-all1-defeat-replay-enter",
                movies::MoviesTarget::CampaignStart {
                    side: Allied,
                    target: movies::CampaignStartTarget::Restart(
                        movies::RestartTarget::DefeatReturnedEnter,
                    ),
                },
            ),
            (
                "campaign-all1-defeat-replay-numpad-enter",
                movies::MoviesTarget::CampaignStart {
                    side: Allied,
                    target: movies::CampaignStartTarget::Restart(
                        movies::RestartTarget::DefeatReturnedNumpadEnter,
                    ),
                },
            ),
            (
                "campaign-all1-defeat-replay-escape",
                movies::MoviesTarget::CampaignStart {
                    side: Allied,
                    target: movies::CampaignStartTarget::Restart(
                        movies::RestartTarget::DefeatReturnedEscape,
                    ),
                },
            ),
            (
                "campaign-all1-defeat-replay-repeated-enter",
                movies::MoviesTarget::CampaignStart {
                    side: Allied,
                    target: movies::CampaignStartTarget::Restart(
                        movies::RestartTarget::DefeatReturnedRepeatedEnter,
                    ),
                },
            ),
            (
                CHECKPOINT_CAMPAIGN_ALL1_RESTATE,
                movies::MoviesTarget::CampaignStart {
                    side: Allied,
                    target: movies::CampaignStartTarget::Restart(movies::RestartTarget::Restate),
                },
            ),
            (
                CHECKPOINT_CAMPAIGN_BACK_RETURN,
                movies::MoviesTarget::CampaignBack,
            ),
        ] {
            let checkpoint = ShellCaptureCheckpoint::parse(name).expect("campaign checkpoint");
            assert_eq!(checkpoint.as_str(), name);
            assert_eq!(checkpoint.movies_target(), Some(target));
        }
        for (name, target, side) in [
            (
                CHECKPOINT_CAMPAIGN_SOV1_OPENING_LOCKED,
                movies::OpeningTarget::Locked,
                Soviet,
            ),
            (
                CHECKPOINT_CAMPAIGN_SOV1_OPENING_PAN,
                movies::OpeningTarget::CameraPan,
                Soviet,
            ),
            (
                CHECKPOINT_CAMPAIGN_SOV1_OPENING_FLASH,
                movies::OpeningTarget::Flash,
                Soviet,
            ),
            (
                CHECKPOINT_CAMPAIGN_SOV1_OPENING_ARROWS,
                movies::OpeningTarget::Arrows,
                Soviet,
            ),
            (
                CHECKPOINT_CAMPAIGN_SOV1_OPENING_UNLOCKED,
                movies::OpeningTarget::Unlocked,
                Soviet,
            ),
            (
                "campaign-sov1-opening-timer-running",
                movies::OpeningTarget::TimerRunning,
                Soviet,
            ),
            (
                "campaign-sov1-opening-timer-expired",
                movies::OpeningTarget::TimerExpired,
                Soviet,
            ),
            (
                "campaign-sov2-radar-alert",
                movies::OpeningTarget::RadarAlert,
                Soviet,
            ),
            (
                "campaign-sov2-radar-restored",
                movies::OpeningTarget::RadarRestored,
                Soviet,
            ),
            (
                CHECKPOINT_CAMPAIGN_SOV1_OPENING_RESTORE_UNLOCKED,
                movies::OpeningTarget::RestoreUnlocked,
                Soviet,
            ),
            (
                CHECKPOINT_CAMPAIGN_ALL1_OPENING_LOAD_UNLOCKED,
                movies::OpeningTarget::LoadUnlockedWhileLocked,
                Allied,
            ),
        ] {
            let checkpoint = ShellCaptureCheckpoint::parse(name).expect("opening checkpoint");
            assert_eq!(checkpoint.as_str(), name);
            assert_eq!(
                checkpoint.movies_target(),
                Some(movies::MoviesTarget::CampaignStart {
                    side,
                    target: movies::CampaignStartTarget::Opening(target),
                })
            );
        }
        for name in [
            "campaign-tut1-first-live-frame",
            "campaign-all1-loading-later-frame",
        ] {
            assert!(ShellCaptureCheckpoint::parse(name).is_err());
        }
    }

    #[test]
    fn allied_opening_tick_capture_keeps_its_production_frame_and_side() {
        use crate::ui::campaign_shell::CampaignSide;
        let checkpoint = ShellCaptureCheckpoint::parse("campaign-all1-opening-tick-180")
            .expect("fixed opening frame");
        assert_eq!(
            checkpoint.movies_target(),
            Some(movies::MoviesTarget::CampaignStart {
                side: CampaignSide::Allied,
                target: movies::CampaignStartTarget::Opening(movies::OpeningTarget::AlliedTick(
                    180
                )),
            })
        );
        for value in ["0", "2501", "-1", "not-a-tick"] {
            assert!(
                ShellCaptureCheckpoint::parse(&format!("campaign-all1-opening-tick-{value}"))
                    .is_err()
            );
        }
    }

    #[test]
    fn soviet_encounter_captures_select_the_real_campaign_and_bound_play_inputs() {
        use crate::ui::campaign_shell::CampaignSide;
        for (name, target) in [
            (
                "campaign-sov1-opening-tick-1500",
                movies::OpeningTarget::SovietTick(1500),
            ),
            (
                "campaign-sov1-navy-tick-6500",
                movies::OpeningTarget::SovietNavy(6500),
            ),
            (
                "campaign-sov07-groups-tick-8000",
                movies::OpeningTarget::SovietGroups(8000),
            ),
            (
                "campaign-sov02-fortresses-tick-8000",
                movies::OpeningTarget::SovietFortresses(8000),
            ),
        ] {
            assert_eq!(
                ShellCaptureCheckpoint::parse(name).unwrap().movies_target(),
                Some(movies::MoviesTarget::CampaignStart {
                    side: CampaignSide::Soviet,
                    target: movies::CampaignStartTarget::Opening(target),
                })
            );
        }
        for invalid in [
            "campaign-sov1-opening-tick-0",
            "campaign-sov1-navy-tick-1099",
            "campaign-sov1-navy-tick-10001",
            "campaign-sov1-navy-tick-not-a-tick",
            "campaign-sov07-groups-tick-300",
            "campaign-sov07-groups-tick-10001",
            "campaign-sov02-fortresses-tick-300",
            "campaign-sov02-fortresses-tick-10001",
        ] {
            assert!(ShellCaptureCheckpoint::parse(invalid).is_err());
        }
    }

    #[test]
    fn network_bounce_checkpoint_round_trips() {
        let checkpoint = ShellCaptureCheckpoint::parse(CHECKPOINT_MAIN_MENU_0XE2_NETWORK_BOUNCE)
            .expect("network bounce checkpoint");
        assert_eq!(
            checkpoint,
            ShellCaptureCheckpoint::MainMenu0xE2NetworkBounce
        );
        assert_eq!(
            checkpoint.as_str(),
            CHECKPOINT_MAIN_MENU_0XE2_NETWORK_BOUNCE
        );
    }

    #[test]
    fn entry_sequence_holds_every_main_menu_entry_tick() {
        let rows = crate::ui::shell::geom::right_panel_rects(
            EXPECTED_WIDTH as i32,
            EXPECTED_HEIGHT as i32,
        )
        .tile_count as u32;
        assert_eq!(
            ShellSlideKind::MainMenu.column(rows, None).total_ticks(),
            u32::from(ENTRY_SEQUENCE_FRAMES)
        );
    }

    #[test]
    fn skirmish_capture_uses_distinct_checkpoint_and_route_budget() {
        let output = new_output_path("skirmish");
        let mut values = valid_args(&output);
        values[1] = OsString::from(CHECKPOINT_SKIRMISH_0X102_STEADY);
        let AppLaunchMode::ShellCapture(request) = parse_launch_args(values).expect("parse") else {
            panic!("expected capture");
        };
        let session = ShellCaptureSession::new(request);
        assert_eq!(
            session.request().checkpoint(),
            ShellCaptureCheckpoint::Skirmish0x102Steady
        );
        assert_eq!(session.timeout(), Duration::from_secs(60));
        assert!(!session.is_entry_sequence());
        assert!(session.skirmish.is_some());
    }

    #[test]
    fn capture_timeout_requires_one_positive_capture_only_value() {
        let output = new_output_path("timeout-args");
        let mut zero = valid_args(&output);
        zero.extend(args(&["--shell-capture-timeout-seconds", "0"]));
        assert!(
            parse_launch_args(zero)
                .unwrap_err()
                .to_string()
                .contains("must be greater than zero")
        );

        let mut duplicate = valid_args(&output);
        duplicate.extend(args(&[
            "--shell-capture-timeout-seconds",
            "3500",
            "--shell-capture-timeout-seconds",
            "3600",
        ]));
        assert!(
            parse_launch_args(duplicate)
                .unwrap_err()
                .to_string()
                .contains("duplicate shell-capture option --shell-capture-timeout-seconds")
        );
        assert!(parse_launch_args(args(&["--shell-capture-timeout-seconds", "3500"])).is_err());
    }

    #[test]
    fn latency_capture_uses_immediate_redraws_but_keeps_its_wall_deadline() {
        let output = new_output_path("latency-cadence");
        let mut values = valid_args(&output);
        let AppLaunchMode::ShellCapture(request) = parse_launch_args(values.clone()).unwrap()
        else {
            panic!("capture")
        };
        let mut still = ShellCaptureSession::new(request);
        let now = Instant::now();
        assert_eq!(
            still.next_wake_deadline_at(now),
            now + Duration::from_millis(8)
        );
        still.frames_seen = still.max_capture_frames() + 1;
        assert!(still.check_capture_budget(Duration::ZERO).is_err());

        values[1] = "online-live-host".into();
        values.extend(args(&[
            "--online-account",
            "Host",
            "--online-latency",
            "--online-frames",
            "9000",
        ]));
        let AppLaunchMode::ShellCapture(request) = parse_launch_args(values).unwrap() else {
            panic!("capture")
        };
        let mut latency = ShellCaptureSession::new(request);
        assert_eq!(latency.next_wake_deadline_at(now), now);
        latency.frames_seen = u32::MAX;
        assert!(latency.check_capture_budget(latency.timeout()).is_ok());
        assert!(
            latency
                .check_capture_budget(latency.timeout() + Duration::from_nanos(1))
                .is_err()
        );
    }

    #[test]
    fn live_network_frame_budget_covers_two_rounds_and_keeps_stills_bounded() {
        let output = new_output_path("live-frame-budget");
        let values = valid_args(&output);
        let AppLaunchMode::ShellCapture(request) = parse_launch_args(values.clone()).unwrap()
        else {
            panic!("capture");
        };
        assert_eq!(
            ShellCaptureSession::new(request).max_capture_frames(),
            10_000
        );

        let mut online = values.clone();
        online[1] = "online-live-host".into();
        online.extend(args(&["--online-account", "Host"]));
        let AppLaunchMode::ShellCapture(request) = parse_launch_args(online.clone()).unwrap()
        else {
            panic!("capture");
        };
        let two_rounds = 2 * request.live_online.as_ref().unwrap().frames;
        let session = ShellCaptureSession::new(request);
        assert_eq!(two_rounds, 14_400);
        assert!(session.max_capture_frames() > two_rounds + 10_000);
        assert_eq!(session.max_capture_frames(), 310_000);
        assert_eq!(session.timeout(), Duration::from_secs(2400));

        let mut lan = values;
        lan[1] = "lan-live-host".into();
        lan.extend(args(&[
            "--lan-bind",
            "127.0.0.1:31000",
            "--lan-peer",
            "127.0.0.1:31010",
        ]));
        let AppLaunchMode::ShellCapture(request) = parse_launch_args(lan).unwrap() else {
            panic!("capture");
        };
        assert_eq!(
            ShellCaptureSession::new(request).max_capture_frames(),
            310_000
        );

        online.extend(args(&["--shell-capture-timeout-seconds", "3500"]));
        let AppLaunchMode::ShellCapture(request) = parse_launch_args(online).unwrap() else {
            panic!("capture");
        };
        let mut session = ShellCaptureSession::new(request);
        assert_eq!(session.timeout(), Duration::from_secs(3500));
        assert_eq!(session.max_capture_frames(), 447_500);
        session.started_at = Some(Instant::now());
        let remaining = session
            .readback_timeout()
            .expect("override reaches readback");
        assert!(remaining > Duration::from_secs(2400));
        assert!(remaining <= Duration::from_secs(3500));
    }

    #[test]
    fn campaign_score_capture_selects_the_synthetic_0x103_route() {
        let output = new_output_path("campaign-score");
        let mut values = valid_args(&output);
        values[1] = OsString::from(CHECKPOINT_CAMPAIGN_SCORE_0X103_STEADY);
        let AppLaunchMode::ShellCapture(request) = parse_launch_args(values).expect("parse") else {
            panic!("expected capture");
        };
        assert_eq!(
            request.checkpoint(),
            ShellCaptureCheckpoint::CampaignScore0x103Steady
        );
        assert_eq!(
            ShellCaptureCheckpoint::parse(CHECKPOINT_CAMPAIGN_SCORE_0X103_STEADY)
                .expect("checkpoint")
                .as_str(),
            CHECKPOINT_CAMPAIGN_SCORE_0X103_STEADY
        );
        let session = ShellCaptureSession::new(request);
        assert_eq!(session.timeout(), Duration::from_secs(60));
        assert!(session.campaign_score.is_some());
        assert!(session.score.is_none());
    }

    #[test]
    fn entry_sequence_manifest_has_exact_ticks_offsets_and_presenter_domain() {
        let output = new_output_path("entry-manifest");
        let mut values = valid_args(&output);
        values[1] = OsString::from(CHECKPOINT_MAIN_MENU_0XE2_ENTRY_SEQUENCE);
        let AppLaunchMode::ShellCapture(request) = parse_launch_args(values).expect("parse") else {
            panic!("expected capture");
        };
        let value = serde_json::to_value(entry_sequence_manifest(
            &request,
            wgpu::TextureFormat::Bgra8UnormSrgb,
            73,
        ))
        .expect("serialize");
        assert_eq!(
            value["schema_version"].as_str(),
            Some("vera20k.shell-entry-sequence-capture.v1")
        );
        assert_eq!(
            value["presenter_domain"].as_str(),
            Some("final-swapchain-after-rgb565")
        );
        assert_eq!(value["generation"].as_u64(), Some(73));
        assert_eq!(value["completion_observed"].as_bool(), Some(true));
        // 18 ticks of 800x600 BGRA8 (9 tile rows + 9, `0x006071E0`).
        assert_eq!(value["payload"]["byte_length"].as_u64(), Some(34_560_000));
        let frames = value["frames"].as_array().expect("frames");
        assert_eq!(frames.len(), 18);
        for (tick, frame) in frames.iter().enumerate() {
            assert_eq!(frame["tick"].as_u64(), Some(tick as u64));
            assert_eq!(frame["byte_offset"].as_u64(), Some(tick as u64 * 1_920_000));
            assert_eq!(frame["byte_length"].as_u64(), Some(1_920_000));
        }
    }

    #[test]
    fn entry_sequence_ledger_rejects_gap_duplicate_and_generation_change() {
        let mut sequence = EntrySequenceState::default();
        let first = EntrySequenceFrameIdentity {
            generation: 5,
            tick: 0,
        };
        sequence.validate_next(first).expect("first");
        sequence.generation = Some(5);
        sequence.expected_next_tick = 1;
        assert!(sequence.validate_next(first).is_err());
        assert!(
            sequence
                .validate_next(EntrySequenceFrameIdentity {
                    generation: 5,
                    tick: 2,
                })
                .is_err()
        );
        assert!(
            sequence
                .validate_next(EntrySequenceFrameIdentity {
                    generation: 6,
                    tick: 1,
                })
                .is_err()
        );
    }

    #[test]
    fn sequence_completion_handoff_returns_before_another_acquire() {
        let steady_output = new_output_path("steady-handoff");
        let AppLaunchMode::ShellCapture(steady) =
            parse_launch_args(valid_args(&steady_output)).expect("steady")
        else {
            panic!("expected steady capture");
        };
        assert_eq!(
            ShellCaptureSession::new(steady).completion_handoff(),
            ShellCompletionHandoff::Continue
        );

        let sequence_output = new_output_path("sequence-handoff");
        let mut values = valid_args(&sequence_output);
        values[1] = OsString::from(CHECKPOINT_MAIN_MENU_0XE2_ENTRY_SEQUENCE);
        let AppLaunchMode::ShellCapture(sequence) = parse_launch_args(values).expect("sequence")
        else {
            panic!("expected sequence capture");
        };
        assert_eq!(
            ShellCaptureSession::new(sequence).completion_handoff(),
            ShellCompletionHandoff::FinalizeExitReturnBeforeAcquire
        );
    }

    #[test]
    fn capture_manifest_serializes_the_strict_validator_schema() {
        let output = new_output_path("manifest");
        let launch = parse_launch_args(valid_args(&output)).expect("parse");
        let AppLaunchMode::ShellCapture(request) = launch else {
            panic!("expected capture");
        };
        let value = serde_json::to_value(capture_manifest(
            &request,
            wgpu::TextureFormat::Bgra8UnormSrgb,
            800 * 600 * 4,
        ))
        .expect("serialize");

        assert_eq!(
            value["schema_version"].as_str(),
            Some("vera20k.shell-capture.v2")
        );
        assert_eq!(value["checkpoint"].as_str(), Some("main-menu-0xe2-steady"));
        assert_eq!(value["surface"]["width"].as_u64(), Some(800));
        assert_eq!(value["surface"]["height"].as_u64(), Some(600));
        assert_eq!(value["surface"]["format"].as_str(), Some("Bgra8UnormSrgb"));
        assert_eq!(value["surface"]["pixel_layout"].as_str(), Some("BGRA8"));
        assert_eq!(value["surface"]["row_order"].as_str(), Some("top-left"));
        assert_eq!(value["surface"]["bytes_per_pixel"].as_u64(), Some(4));
        assert_eq!(value["surface"]["row_stride"].as_u64(), Some(3200));
        assert_eq!(value["cursor"]["x"].as_u64(), Some(400));
        assert_eq!(value["cursor"]["y"].as_u64(), Some(300));
        assert_eq!(
            value["cursor"]["policy"].as_str(),
            Some("software-composited")
        );
        assert_eq!(value["shell"]["screen"].as_str(), Some("main-menu"));
        assert_eq!(value["shell"]["dialog_resource_id"].as_u64(), Some(0x00E2));
        assert_eq!(
            value["shell"]["movie_owner"].as_str(),
            Some("main-menu-0xe2")
        );
        assert_eq!(value["shell"]["movie_base"].as_str(), Some("ra2ts-l"));
        assert_eq!(
            value["shell"]["title_terminal_persistent"].as_bool(),
            Some(true)
        );
        assert_eq!(value["frame"]["path"].as_str(), Some("frame.bgra"));
        assert_eq!(value["frame"]["byte_length"].as_u64(), Some(1_920_000));
    }

    #[test]
    fn unsupported_resolution_fails_closed() {
        let output = new_output_path("resolution");
        let mut values = valid_args(&output);
        let width = values
            .iter()
            .position(|value| value == "--width")
            .expect("width");
        values[width + 1] = OsString::from("1024");
        let err = parse_launch_args(values).expect_err("must reject");
        assert!(err.to_string().contains("requires exactly 800x600"));
    }

    #[test]
    fn duplicate_option_fails_closed() {
        let output = new_output_path("duplicate");
        let mut values = valid_args(&output);
        values.extend(args(&["--width", "800"]));
        let err = parse_launch_args(values).expect_err("must reject");
        assert!(err.to_string().contains("duplicate"));
    }

    #[test]
    fn existing_output_directory_is_never_overwritten() {
        let output = new_output_path("existing");
        fs::create_dir(&output).expect("create owned temp dir");
        let err = parse_launch_args(valid_args(&output)).expect_err("must reject");
        assert!(err.to_string().contains("already exists"));
        fs::remove_dir(&output).expect("remove owned temp dir");
    }

    fn ready_snapshot() -> MainMenuCaptureSnapshot {
        MainMenuCaptureSnapshot {
            width: 800,
            height: 600,
            main_menu_screen: true,
            shell_failed: false,
            single_player_active: false,
            skirmish_active: false,
            legacy_skirmish_setup_active: false,
            modal_open: false,
            quit_active: false,
            first_paint_slide_active: false,
            active_slide_is_main_menu: true,
            title_terminal_persistent: true,
            movie_loaded: true,
            movie_owner_is_main_menu: true,
            movie_base_is_large: true,
            movie_base_matches_layout: true,
            chrome_loaded: true,
            software_cursor_active: true,
            cursor_x: 400.0,
            cursor_y: 300.0,
        }
    }

    #[test]
    fn active_wave_waits_without_weakening_identity_checks() {
        let mut snapshot = ready_snapshot();
        snapshot.first_paint_slide_active = true;
        snapshot.movie_loaded = false;
        snapshot.movie_owner_is_main_menu = false;
        assert!(!steady_main_menu_capture_ready(snapshot).expect("wait"));
    }

    #[test]
    fn first_ordinary_steady_frame_is_capture_ready() {
        assert!(steady_main_menu_capture_ready(ready_snapshot()).expect("ready"));
    }

    #[test]
    fn latency_main_menu_entry_and_return_accept_the_640_shell_movie() {
        let mut snapshot = ready_snapshot();
        snapshot.width = 640;
        snapshot.height = 480;
        snapshot.movie_base_is_large = false;
        assert!(steady_main_menu_capture_ready_for(snapshot, true).unwrap());
        assert!(
            steady_main_menu_capture_ready(snapshot).is_err(),
            "ordinary reference captures remain fixed"
        );
        snapshot.movie_base_matches_layout = false;
        assert!(
            steady_main_menu_capture_ready_for(snapshot, true).is_err(),
            "the actual 640 shell movie identity is still required"
        );
    }

    #[test]
    fn running_title_waits_for_the_retained_terminal_frame() {
        let mut snapshot = ready_snapshot();
        snapshot.title_terminal_persistent = false;
        assert!(!steady_main_menu_capture_ready(snapshot).expect("wait"));
    }

    #[test]
    fn wrong_movie_owner_is_invalid_not_waiting() {
        let mut snapshot = ready_snapshot();
        snapshot.movie_owner_is_main_menu = false;
        let err = steady_main_menu_capture_ready(snapshot).expect_err("must reject");
        assert!(err.to_string().contains("session identity"));
    }
}
