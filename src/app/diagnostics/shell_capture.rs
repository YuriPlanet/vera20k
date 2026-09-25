//! Explicit one-shot capture mode for exact shell certification.
//!
//! The capture session is app-level diagnostic state. It never enters the
//! deterministic simulation, never synthesizes OS input, and never rebuilds a
//! shell in a second renderer. It waits for the production main-menu dispatcher
//! to reach one ordinary steady frame, then records the final swapchain bytes.

use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;

use crate::app::AppState;
use crate::app::frontend::main_menu_shell_render::Ra2tsDialogOwner;
use crate::app::frontend::shell_transition::{MainMenuEntryPresentToken, ShellSlideKind};
use crate::render::frame_readback::PendingBgra8Readback;
use crate::ui::game_screen::GameScreen;
use crate::ui::main_menu_shell::MainMenuMovieBase;
use crate::ui::shell::static_reveal::Kind1PaintWindow;

mod movies;
mod skirmish;
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
const CHECKPOINT_CAMPAIGN_0X94_ENTRY_PREFIX: &str = "campaign-0x94-entry-tick-";
const CHECKPOINT_LOAD_SAVED_GAME_0XB7_STEADY: &str = "load-saved-game-0xb7-steady";
const CHECKPOINT_LOAD_SAVED_GAME_0XB7_ENTRY_PREFIX: &str = "load-saved-game-0xb7-entry-tick-";
const CHECKPOINT_OPTIONS_0XD5_STEADY: &str = "options-0xd5-steady";
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
/// Slider press points of the retail comparison stills.
const CAMPAIGN_SLIDER_LEFT_POINT: (i32, i32) = (190, 275);
const CAMPAIGN_SLIDER_RIGHT_POINT: (i32, i32) = (440, 275);
const CHECKPOINT_CREDITS_ROLL_FRAME_PREFIX: &str = "credits-roll-frame-";
const CHECKPOINT_SNEAK_PEEK_FRAME_PREFIX: &str = "sneak-peek-frame-";
const CHECKPOINT_MAIN_MENU_0XE2_SLIDE_OUT_PREFIX: &str = "main-menu-0xe2-slide-out-tick-";
const CHECKPOINT_MOVIE_LIST_0X129_SLIDE_OUT_PREFIX: &str = "movie-list-0x129-slide-out-tick-";
const CHECKPOINT_SKIRMISH_0X102_BACK_SLIDE_OUT_PREFIX: &str = "skirmish-0x102-back-slide-out-tick-";
const CHECKPOINT_SKIRMISH_0X102_ENTRY_PREFIX: &str = "skirmish-0x102-entry-tick-";
const CHECKPOINT_SKIRMISH_0X6B_STEADY: &str = "skirmish-0x6b-steady";
const CHECKPOINT_SKIRMISH_0X6B_ENTRY_PREFIX: &str = "skirmish-0x6b-entry-tick-";
const CHECKPOINT_SKIRMISH_0X102_CHOOSE_MAP_RETURN: &str = "skirmish-0x102-choose-map-return";
const CHECKPOINT_SKIRMISH_START_BLANK: &str = "skirmish-start-blank";
const CHECKPOINT_SKIRMISH_LOADING_FIRST_FRAME: &str = "skirmish-loading-first-frame";
const CHECKPOINT_SKIRMISH_0X6B_EJECT_BOX: &str = "skirmish-0x6b-eject-box";
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
    /// Its entry slide held at one tick (`campaign-0x94-entry-tick-<N>`).
    Campaign0x94Entry(u32),
    /// Single Player -> Load Saved Game: `0xB7` settled. Needs a readable
    /// save in the saves directory.
    LoadSavedGame0xB7Steady,
    /// Its entry slide held at one tick
    /// (`load-saved-game-0xb7-entry-tick-<N>`).
    LoadSavedGame0xB7Entry(u32),
    /// Main Menu -> Options: `0xD5` settled.
    Options0xD5Steady,
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
}

impl ShellCaptureCheckpoint {
    fn parse(value: &str) -> Result<Self> {
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
            let last = kind.column(rows).total_ticks() - 1;
            ensure!(tick <= last, "{value}: the slide shows ticks 0..={last}");
            return Ok(checkpoint(tick));
        }
        match value {
            CHECKPOINT_MAIN_MENU_0XE2_STEADY => Ok(Self::MainMenu0xE2Steady),
            CHECKPOINT_MAIN_MENU_0XE2_ENTRY_SEQUENCE => Ok(Self::MainMenu0xE2EntrySequence),
            CHECKPOINT_SKIRMISH_0X102_STEADY => Ok(Self::Skirmish0x102Steady),
            CHECKPOINT_SKIRMISH_0X6B_STEADY => Ok(Self::Skirmish0x6BSteady),
            CHECKPOINT_SKIRMISH_0X102_CHOOSE_MAP_RETURN => Ok(Self::Skirmish0x102ChooseMapReturn),
            CHECKPOINT_SKIRMISH_START_BLANK => Ok(Self::SkirmishStartBlank),
            CHECKPOINT_SKIRMISH_LOADING_FIRST_FRAME => Ok(Self::SkirmishLoadingFirstFrame),
            CHECKPOINT_SKIRMISH_0X6B_EJECT_BOX => Ok(Self::Skirmish0x6BEjectBox),
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
            CHECKPOINT_LOAD_SAVED_GAME_0XB7_STEADY => Ok(Self::LoadSavedGame0xB7Steady),
            CHECKPOINT_OPTIONS_0XD5_STEADY => Ok(Self::Options0xD5Steady),
            CHECKPOINT_MAIN_MENU_0XE2_NETWORK_BOUNCE => Ok(Self::MainMenu0xE2NetworkBounce),
            CHECKPOINT_WOL_0X10E_STEADY => Ok(Self::Wol0x10ESteady),
            CHECKPOINT_WOL_0X10E_HOVER_QUICK_MATCH => Ok(Self::Wol0x10EHoverQuickMatch),
            CHECKPOINT_WOL_0X10E_API_MISSING => Ok(Self::Wol0x10EApiMissing),
            CHECKPOINT_WOL_0X10E_BACK => Ok(Self::Wol0x10EBack),
            _ => {
                if let Some(index) = OPTIONS_0XD5_HOVER_CHECKPOINTS
                    .iter()
                    .position(|(name, _)| *name == value)
                {
                    return Ok(Self::Options0xD5Hover(index));
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
            Self::Campaign0x94Entry(_) => "campaign-0x94-entry",
            Self::LoadSavedGame0xB7Steady => CHECKPOINT_LOAD_SAVED_GAME_0XB7_STEADY,
            Self::LoadSavedGame0xB7Entry(_) => "load-saved-game-0xb7-entry",
            Self::Options0xD5Steady => CHECKPOINT_OPTIONS_0XD5_STEADY,
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
            Self::Skirmish0x6BSteady => CHECKPOINT_SKIRMISH_0X6B_STEADY,
            Self::Skirmish0x6BEntry(_) => "skirmish-0x6b-entry",
            Self::Skirmish0x102ChooseMapReturn => CHECKPOINT_SKIRMISH_0X102_CHOOSE_MAP_RETURN,
            Self::SkirmishStartBlank => CHECKPOINT_SKIRMISH_START_BLANK,
            Self::SkirmishLoadingFirstFrame => CHECKPOINT_SKIRMISH_LOADING_FIRST_FRAME,
            Self::Skirmish0x6BEjectBox => CHECKPOINT_SKIRMISH_0X6B_EJECT_BOX,
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
                entry_tick: None,
            },
            Self::Campaign0x94SliderLeft => movies::MoviesTarget::Campaign0x94 {
                press: Some(CAMPAIGN_SLIDER_LEFT_POINT),
                entry_tick: None,
            },
            Self::Campaign0x94SliderRight => movies::MoviesTarget::Campaign0x94 {
                press: Some(CAMPAIGN_SLIDER_RIGHT_POINT),
                entry_tick: None,
            },
            Self::Campaign0x94Entry(tick) => movies::MoviesTarget::Campaign0x94 {
                press: None,
                entry_tick: Some(tick),
            },
            Self::LoadSavedGame0xB7Steady => {
                movies::MoviesTarget::LoadSavedGame0xB7 { entry_tick: None }
            }
            Self::LoadSavedGame0xB7Entry(tick) => movies::MoviesTarget::LoadSavedGame0xB7 {
                entry_tick: Some(tick),
            },
            Self::MainMenu0xE2NetworkBounce => movies::MoviesTarget::NetworkBounce,
            Self::Wol0x10ESteady => movies::MoviesTarget::WolWelcome {
                entry_tick: None,
                hover: None,
                press: movies::WolPress::None,
            },
            Self::Wol0x10EEntry(tick) => movies::MoviesTarget::WolWelcome {
                entry_tick: Some(tick),
                hover: None,
                press: movies::WolPress::None,
            },
            Self::Wol0x10EHoverQuickMatch => movies::MoviesTarget::WolWelcome {
                entry_tick: None,
                hover: Some(WOL_QUICK_MATCH_POINT),
                press: movies::WolPress::None,
            },
            Self::Wol0x10EApiMissing => movies::MoviesTarget::WolWelcome {
                entry_tick: None,
                hover: None,
                press: movies::WolPress::MyInformation,
            },
            Self::Wol0x10EBack => movies::MoviesTarget::WolWelcome {
                entry_tick: None,
                hover: None,
                press: movies::WolPress::MainMenu,
            },
            Self::Options0xD5Steady => movies::MoviesTarget::Options0xD5 {
                entry_tick: None,
                hover: None,
            },
            Self::Options0xD5Entry(tick) => movies::MoviesTarget::Options0xD5 {
                entry_tick: Some(tick),
                hover: None,
            },
            Self::Options0xD5Hover(index) => movies::MoviesTarget::Options0xD5 {
                entry_tick: None,
                hover: Some(OPTIONS_0XD5_HOVER_CHECKPOINTS[index].1),
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
}

impl ShellCaptureRequest {
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
        first == OsString::from(CAPTURE_FLAG),
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
            _ => bail!("unknown shell-capture option {flag:?}"),
        }
    }

    let width = width.context("missing required --width")?;
    let height = height.context("missing required --height")?;
    let cursor_x = cursor_x.context("missing required --cursor-x")?;
    let cursor_y = cursor_y.context("missing required --cursor-y")?;
    let output_dir = output_dir.context("missing required --output")?;

    ensure!(
        width == EXPECTED_WIDTH && height == EXPECTED_HEIGHT,
        "checkpoint {} requires exactly {EXPECTED_WIDTH}x{EXPECTED_HEIGHT}, got {width}x{height}",
        checkpoint.as_str()
    );
    ensure!(
        cursor_x == EXPECTED_CURSOR_X && cursor_y == EXPECTED_CURSOR_Y,
        "checkpoint {} requires neutral cursor ({EXPECTED_CURSOR_X},{EXPECTED_CURSOR_Y}), got \
         ({cursor_x},{cursor_y})",
        checkpoint.as_str()
    );
    validate_new_output_dir(&output_dir)?;

    Ok(AppLaunchMode::ShellCapture(ShellCaptureRequest {
        checkpoint,
        width,
        height,
        cursor_x,
        cursor_y,
        output_dir,
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
            shell_failed: state.frontend.main_menu_shell_failed,
            single_player_active: state.frontend.shell_route.single_player(),
            skirmish_active: state.frontend.shell_route.skirmish()
                || state.frontend.dev_skirmish_shell_enabled,
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
            chrome_loaded: state.frontend.main_menu_shell_chrome.is_some(),
            software_cursor_active: state.use_software_cursor(),
            cursor_x: state.match_state.input.cursor_x,
            cursor_y: state.match_state.input.cursor_y,
        }
    }
}

fn steady_main_menu_capture_ready(snapshot: MainMenuCaptureSnapshot) -> Result<bool> {
    ensure!(
        snapshot.width == EXPECTED_WIDTH && snapshot.height == EXPECTED_HEIGHT,
        "capture surface changed from 800x600 to {}x{}",
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
        snapshot.movie_loaded && snapshot.movie_owner_is_main_menu && snapshot.movie_base_is_large,
        "main-menu RA2TS_L session identity is not ready"
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
            ShellCaptureCheckpoint::Skirmish0x102Entry(tick) => {
                Some(skirmish::SkirmishCapture::entry(tick))
            }
            _ => None,
        };
        let movies = request
            .checkpoint
            .movies_target()
            .map(movies::MoviesCapture::new);
        Self {
            request,
            started_at: None,
            frames_seen: 0,
            readback_started: false,
            entry_sequence,
            skirmish,
            movies,
            outcome: None,
        }
    }

    pub(crate) fn request(&self) -> &ShellCaptureRequest {
        &self.request
    }

    pub(crate) fn is_entry_sequence(&self) -> bool {
        self.entry_sequence.is_some()
    }

    pub(crate) fn completion_handoff(&self) -> ShellCompletionHandoff {
        if self.is_entry_sequence() {
            ShellCompletionHandoff::FinalizeExitReturnBeforeAcquire
        } else {
            ShellCompletionHandoff::Continue
        }
    }

    pub(crate) fn prepare_state(&mut self, state: &mut AppState) {
        state.match_state.input.cursor_x = self.request.cursor_x as f32;
        state.match_state.input.cursor_y = self.request.cursor_y as f32;
        self.started_at = Some(Instant::now());
    }

    fn timeout(&self) -> Duration {
        if self.skirmish.is_some() || self.movies.is_some() {
            Duration::from_secs(60)
        } else {
            CAPTURE_TIMEOUT
        }
    }

    fn steady_ready(&self, state: &AppState) -> Result<bool> {
        if let Some(capture) = &self.movies {
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
        if let Some(capture) = &mut self.skirmish {
            capture.after_present(state, rendered, self.frames_seen)?;
        }
        if let Some(capture) = &mut self.movies {
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
        ensure!(
            self.frames_seen <= MAX_CAPTURE_FRAMES,
            "shell capture exceeded {MAX_CAPTURE_FRAMES} frames"
        );
        ensure!(
            started_at.elapsed() <= self.timeout(),
            "shell capture timed out after {} seconds",
            self.timeout().as_secs()
        );

        if self.is_entry_sequence() {
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
            started_at.elapsed() <= CAPTURE_TIMEOUT,
            "shell capture timed out after {} seconds",
            CAPTURE_TIMEOUT.as_secs()
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
        Instant::now() + CAPTURE_FRAME_INTERVAL
    }

    pub(crate) fn complete(
        &mut self,
        state: &AppState,
        surface_format: wgpu::TextureFormat,
        pixels: &[u8],
    ) -> Result<()> {
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

        fs::create_dir(self.request.output_dir()).with_context(|| {
            format!(
                "create immutable shell-capture directory {}",
                self.request.output_dir().display()
            )
        })?;
        let frame_path = self.request.output_dir().join(FRAME_FILE_NAME);
        write_new_file(&frame_path, pixels)?;

        let mut manifest_bytes = match (&self.skirmish, &self.movies) {
            (Some(capture), _) => serde_json::to_vec_pretty(&capture.manifest(
                &self.request,
                surface_format,
                pixels,
                self.frames_seen,
            )),
            (None, Some(capture)) => serde_json::to_vec_pretty(&capture.manifest(
                &self.request,
                surface_format,
                pixels,
                self.frames_seen,
            )),
            (None, None) => serde_json::to_vec_pretty(&capture_manifest(
                &self.request,
                surface_format,
                pixels.len() as u64,
            )),
        }
        .context("serialize shell-capture manifest")?;
        manifest_bytes.push(b'\n');
        let manifest_path = self.request.output_dir().join(MANIFEST_FILE_NAME);
        write_new_file(&manifest_path, &manifest_bytes)?;

        self.outcome = Some(Ok(()));
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
            let remaining = CAPTURE_TIMEOUT
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
        for (name, _) in OPTIONS_0XD5_HOVER_CHECKPOINTS {
            let checkpoint = ShellCaptureCheckpoint::parse(name).expect("hover checkpoint");
            assert_eq!(checkpoint.as_str(), name);
        }
        assert!(ShellCaptureCheckpoint::parse("options-0xd5-hover-nothing").is_err());
    }

    #[test]
    fn wol_checkpoints_round_trip() {
        for name in [
            CHECKPOINT_WOL_0X10E_STEADY,
            CHECKPOINT_WOL_0X10E_HOVER_QUICK_MATCH,
            CHECKPOINT_WOL_0X10E_API_MISSING,
            CHECKPOINT_WOL_0X10E_BACK,
        ] {
            let checkpoint = ShellCaptureCheckpoint::parse(name).expect("WOL checkpoint");
            assert_eq!(checkpoint.as_str(), name);
        }
        assert_eq!(
            ShellCaptureCheckpoint::parse("wol-0x10e-entry-tick-17").expect("last tick"),
            ShellCaptureCheckpoint::Wol0x10EEntry(17)
        );
        assert!(ShellCaptureCheckpoint::parse("wol-0x10e-entry-tick-18").is_err());
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
            ShellSlideKind::MainMenu.column(rows).total_ticks(),
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
