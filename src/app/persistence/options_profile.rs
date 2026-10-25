//! Process-owned retail Options/Video/Audio profile transaction.
//!
//! This module translates the active-YR `OptionsClass` profile boundary into
//! one ordinary Rust value. It owns exact defaults, typed `VERA.INI` loading,
//! separate game-pair resolution and frontend selection, native-shaped formatting, and the one-read /
//! one-write preservation-safe commit. Runtime consumers remain in their
//! existing app, presentation, and audio owners.

use std::io;
use std::path::Path;

use crate::app::frontend::startup_options::{RetailStartupOptions, ScreenSize};
use crate::rules::ini_parser::{IniFile, IniSection};
use crate::util::ini_writer::set_ini_values;

use super::profile_file;

/// How many builds one Shift-click queues when `[VERA] ShiftQueueCount=` is
/// absent, and the inclusive bounds a configured value is clamped to.
///
/// Both bounds exist because `read_int` answers 0 for a present-but-nonnumeric
/// value: zero would make the click silently queue nothing, and an unbounded
/// upper end would let one press flood the strip. The maximum is the stock
/// `[General] MaximumQueuedObjects=` (29) plus the active build, so one press
/// can at most fill an otherwise empty strip; the factory's own check still
/// refuses anything beyond its real cap.
pub(crate) const DEFAULT_SHIFT_QUEUE_COUNT: u32 = 5;
pub(crate) const SHIFT_QUEUE_COUNT_RANGE: std::ops::RangeInclusive<u32> = 1..=30;

const OPTIONS_SECTION: &str = "Options";
const VIDEO_SECTION: &str = "Video";
const AUDIO_SECTION: &str = "Audio";
const WONLINE_SECTION: &str = "WOnline";
const NETWORK_SECTION: &str = "Network";
/// VERA's own display preferences. gamemd never reads this section.
const VERA_SECTION: &str = "VERA";
/// Destination units `OptionsClass__ReadFromINI` passes for NetID (`push 0xC8`).
const NET_ID_UNITS: usize = 0xC8;
/// Destination units `[VERA] Display=`'s ReadString copies into. A VERA key, so
/// the capacity is ours: a device name is at most `\\.\DISPLAY` plus a few
/// digits, and 64 leaves room for a backend that names screens differently.
const DISPLAY_CAPACITY: usize = 64;
const SCREEN_SIZE_UNSET: i32 = -1;
const DEFAULT_SCREEN_WIDTH: i32 = 800;
const DEFAULT_SCREEN_HEIGHT: i32 = 600;
pub(crate) const RETAIL_SHELL_SIZE: ScreenSize = ScreenSize {
    width: 800,
    height: 600,
};

/// The process-lifetime subset of active-YR `OptionsClass` owned by
/// `VERA.INI` `[Options]`, `[Video]`, and `[Audio]`.
///
/// Signed integer and raw volume fields deliberately retain values that are
/// unsafe or outside a current UI control's range. Consumers clamp only at
/// their platform boundary; the profile can therefore round-trip what retail
/// loaded without silently becoming a second settings authority.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RetailOptionsProfile {
    // [Options]
    pub(crate) game_speed: i32,
    pub(crate) difficulty: i32,
    pub(crate) camp_difficulty: i32,
    pub(crate) scroll_method: i32,
    pub(crate) scroll_rate: i32,
    pub(crate) auto_scroll: bool,
    pub(crate) detail_level: i32,
    pub(crate) sidebar_cameo_text: bool,
    pub(crate) unit_action_lines: bool,
    pub(crate) show_hidden: bool,
    pub(crate) tooltips: bool,

    // [Video]
    pub(crate) screen_width: i32,
    pub(crate) screen_height: i32,
    pub(crate) stretch_movies: bool,
    /// Read and retained, but deliberately omitted by the native writer. The
    /// main-menu code HIRES toggles it.
    pub(crate) allow_hi_res_modes: bool,
    /// Native stores this outside the contiguous Options object. The main-menu
    /// code TOGGLE toggles it; it is never serialized.
    pub(crate) allow_mode_toggle: bool,
    /// Read and retained, but deliberately omitted by the native writer.
    pub(crate) allow_vram_sidebar: bool,

    // [Audio]
    pub(crate) sound_volume: f32,
    pub(crate) voice_volume: f32,
    pub(crate) score_volume: f32,
    pub(crate) is_score_repeat: bool,
    pub(crate) is_score_shuffle: bool,
    pub(crate) sound_latency: u16,
    pub(crate) in_game_music: bool,

    // [Network] NetID: campaign movie unlock progress (OptionsClass +0x4C
    // Soviet, +0x50 Allied), -1 when none. The obfuscated key is decoded by
    // `ReadFromINI` `0x005FABA6..0x005FAC54` and written by `WriteToINI`
    // `0x005FAF9E`; Play_Movie raises it through `0x005FBF80`.
    pub(crate) movie_progress: crate::ui::movies_credits_shell::MovieProgress,

    // OptionsClass +0x4A/+0x54/+0x58; ReadFromINI 5FAC88..5FACDE.
    pub(crate) network_socket: u16,
    pub(crate) network_card: i32,
    pub(crate) network_destination: String,
    // SessionClass__ReadMultiPlayerSettings 6981E6..698228 and D7 560480.
    pub(crate) network_send_delay: bool,
    pub(crate) network_port: i32,

    // [WOnline]'s nine WOL preferences, outside OptionsClass: the same
    // profile object (RA2MD.INI in native) `WOL_Main`'s reader `0x0077DED0` reads at every WOL
    // entry and WOL Options' writer `0x0077DFD0` writes.
    pub(crate) wol_preferences: crate::ui::wol_options::WolPreferences,

    // [VERA]: VERA's display preferences, not part of OptionsClass.
    /// Whether the window opens full screen. While the game runs the window
    /// itself owns this; `App::follow_window_full_screen` copies its state
    /// back every event-loop pass, so each write saves what the player sees.
    pub(crate) full_screen: bool,
    /// The Options page's battle zoom.
    pub(crate) zoom: crate::ui::main_menu_dialogs::options::Zoom,
    /// Whether battles keep gamemd's 4:3 screen shape, with bars on a window
    /// of any other shape, instead of filling the window.
    pub(crate) original_shape: bool,
    /// The Options page's battle sidebar size.
    pub(crate) sidebar: crate::ui::main_menu_dialogs::options::SidebarSize,
    /// `[VERA] YROriginalStyle=`: whether the startup splash and the menu
    /// movie use Yuri's Revenge's art. Off by default, so new profiles show
    /// Red Alert 2's. Only those two read it; see
    /// [`LauncherCheckboxId::YrOriginalStyle`](crate::ui::main_menu_dialogs::options::LauncherCheckboxId::YrOriginalStyle).
    pub(crate) yr_original_style: bool,
    /// `[VERA] RenderInterpolation=`: client-only motion smoothing. Off by
    /// default, so new profiles draw the original tick-stepped motion; it never
    /// changes the simulation rate or saves.
    pub(crate) render_interpolation: bool,
    /// `[VERA] MouseWheelZoom=`: let the mouse wheel change the battle's
    /// view distance. Off by default; this is a local camera preference.
    pub(crate) mouse_wheel_zoom: bool,
    /// `[VERA] ShiftQueue=`: whether a Shift-click on a mobile strip queues
    /// [`Self::shift_queue_count`] builds: the Options page's Shift-Click
    /// Batch Queue. Off by default, so Shift + left click queues one copy as
    /// in gamemd, whose left-click ignores Shift (`0x006AAD66` reads the
    /// modifier for the right button only).
    pub(crate) shift_queue: bool,
    /// `[VERA] ShiftQueueCount=`: how many builds one Shift-click queues on a
    /// mobile strip while [`Self::shift_queue`] is on. VERA's own convenience;
    /// gamemd never reads this section. Clamped by [`SHIFT_QUEUE_COUNT_RANGE`].
    pub(crate) shift_queue_count: u32,
    /// `[VERA] WolTestService=`: Internet answers from VERA's stand-in WOL
    /// service instead of showing `TXT_APIMISSING`: the Options page's
    /// developer box. Off by default; no real service exists yet.
    pub(crate) wol_test_service: bool,
    /// `[VERA] SoloSkirmish=`: Start accepts a skirmish whose AI rows are all
    /// `None`, for an undisturbed test match. Off by default, so retail's
    /// "at least two players" gate stays.
    pub(crate) solo_skirmish: bool,
    /// `[VERA] TranslateChat=`: the language received online chat is
    /// translated into, or `None`, the default, for off. Only the `/translate`
    /// chat command changes it ([`Self::commit_chat_translation`]).
    pub(crate) translate_chat: Option<crate::ui::chat_translation::ChatLanguage>,
    /// `[VERA] Display=`: the screen the game opens on, by the window system's
    /// own name (`\\.\DISPLAY1`). Absent or empty means the system's choice,
    /// which is how every earlier build behaved. A name no longer attached
    /// stays here and opens on the system's choice, like [`Self::zoom`]'s
    /// unknown steps.
    pub(crate) display: crate::ui::main_menu_dialogs::options::DisplayTarget,
}

/// The two process-start products obtained from one physical `VERA.INI`
/// snapshot.
///
/// Native resolves the game preference after an early Video read, separately
/// selects the frontend window, then rereads the complete profile later.
/// Keeping both products explicit prevents game settings from sizing the shell.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RetailOptionsLoad {
    pub(crate) retained_profile: RetailOptionsProfile,
    pub(crate) startup_shell_screen: ScreenSize,
}

impl Default for RetailOptionsProfile {
    fn default() -> Self {
        // Retail provenance: Options profile defaults —
        // `OptionsClass__SetDefaults` @ `0x005FA350`.
        Self {
            game_speed: 3,
            difficulty: 1,
            // Native relies on the static object's zero initialization for
            // this one persisted field rather than storing it in the body.
            camp_difficulty: 0,
            scroll_method: 0,
            scroll_rate: 3,
            auto_scroll: true,
            detail_level: 2,
            sidebar_cameo_text: true,
            unit_action_lines: true,
            show_hidden: false,
            tooltips: true,
            screen_width: SCREEN_SIZE_UNSET,
            screen_height: SCREEN_SIZE_UNSET,
            stretch_movies: false,
            allow_hi_res_modes: false,
            allow_mode_toggle: false,
            allow_vram_sidebar: false,
            sound_volume: 0.7,
            voice_volume: 0.7,
            score_volume: 0.4,
            is_score_repeat: false,
            is_score_shuffle: false,
            sound_latency: 9,
            in_game_music: true,
            movie_progress: crate::ui::movies_credits_shell::MovieProgress::default(),
            network_socket: u16::MAX,
            network_card: 0,
            network_destination: String::new(),
            network_send_delay: false,
            network_port: 0,
            wol_preferences: Default::default(),
            full_screen: false,
            zoom: crate::ui::main_menu_dialogs::options::Zoom::Auto,
            original_shape: false,
            sidebar: crate::ui::main_menu_dialogs::options::SidebarSize::Normal,
            yr_original_style: false,
            render_interpolation: false,
            mouse_wheel_zoom: false,
            shift_queue: false,
            shift_queue_count: DEFAULT_SHIFT_QUEUE_COUNT,
            wol_test_service: false,
            solo_skirmish: false,
            translate_chat: None,
            display: crate::ui::main_menu_dialogs::options::DisplayTarget::Auto,
        }
    }
}

/// Fully formatted owned sections, in native write order.
///
/// The read-only Video `Allow*` values are intentionally absent. Keeping the
/// groups explicit makes it impossible for the filesystem transaction to
/// accidentally emit a partially modeled settings snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FormattedProfileSections {
    pub(crate) options: [(&'static str, String); 11],
    pub(crate) video: [(&'static str, String); 3],
    pub(crate) audio: [(&'static str, String); 7],
    pub(crate) network: [(&'static str, String); 4],
    /// VERA's own section, written after the native ones.
    pub(crate) vera: [(&'static str, String); 12],
}

impl RetailOptionsLoad {
    /// Replay startup with no profile snapshot.
    ///
    /// This is used when configuration is unavailable and by sealed capture
    /// after passing exact constructor defaults instead of operator argv.
    pub(crate) fn without_settings(startup: &RetailStartupOptions) -> Self {
        Self::from_ini_snapshot(startup, None)
    }

    /// Read and parse one physical `VERA.INI` snapshot, then replay native's
    /// early Video-only read and later complete profile read over it.
    ///
    /// Missing/unreadable/malformed input is non-fatal and follows the same
    /// no-snapshot path. Capture callers intentionally use
    /// [`Self::without_settings`] so user profile state cannot contaminate sealed
    /// capture output.
    pub(crate) fn from_settings(ra2_dir: &Path, startup: &RetailStartupOptions) -> Self {
        Self::from_settings_with(ra2_dir, startup, |path| std::fs::read(path))
    }

    /// Injectable form of [`Self::from_settings`] used to prove that both
    /// semantic reads share one physical filesystem read.
    fn from_settings_with<Read>(ra2_dir: &Path, startup: &RetailStartupOptions, read: Read) -> Self
    where
        Read: FnOnce(&Path) -> io::Result<Vec<u8>>,
    {
        let path = profile_file::path(ra2_dir);
        let bytes = match read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Self::without_settings(startup);
            }
            Err(error) => {
                log::warn!(
                    "Could not read {} for retail options: {error}",
                    path.display()
                );
                return Self::without_settings(startup);
            }
        };
        let ini = match IniFile::from_bytes(&bytes) {
            Ok(ini) => ini,
            Err(error) => {
                log::warn!(
                    "Could not parse {} for retail options: {error}",
                    path.display()
                );
                return Self::without_settings(startup);
            }
        };
        Self::from_ini_snapshot(startup, Some(&ini))
    }

    fn from_ini_snapshot(startup: &RetailStartupOptions, ini: Option<&IniFile>) -> Self {
        let mut retained_profile = RetailOptionsProfile::from_startup(startup);

        // Retail provenance: WinMain's pre-window Video-only read and paired
        // fallback — `WinMain` @ `0x006BB9A0`, branch
        // `0x006BD94A..0x006BD9B5`.
        if let Some(ini) = ini {
            retained_profile.apply_startup_video_ini(ini);
        }
        retained_profile.resolve_screen_size();
        // WinMain 006BD9B5 selects the independent shell pair A8EB8C/90
        // (defaults 800x600 at 005FA3D0), not the configured game pair.
        // Explicit AllowModeToggle takes the separate 640x480 startup branch.
        let allow_mode_toggle = ini
            .and_then(|ini| ini.section(VIDEO_SECTION))
            .is_some_and(|video| video.read_bool("AllowModeToggle", false));
        let startup_shell_screen = if allow_mode_toggle {
            ScreenSize {
                width: 640,
                height: 480,
            }
        } else {
            RETAIL_SHELL_SIZE
        };

        // Retail provenance: the complete `OptionsClass__ReadFromINI` @
        // `0x005FA620`, called by `Init_Game` at `0x0052C630`. Reusing the
        // parsed immutable snapshot preserves the ordering without introducing
        // a second filesystem authority.
        if let Some(ini) = ini {
            retained_profile.apply_ini(ini);
        }

        Self {
            retained_profile,
            startup_shell_screen,
        }
    }
}

impl RetailOptionsProfile {
    /// `OptionsClass::Set_Score_Volume` at `0x005FA4A0` stores an upper-only
    /// clamped f32. Reuse the INI reader's normalization so live setters retain
    /// negative and unordered inputs instead of copying the output clamp.
    pub(crate) fn set_score_volume(&mut self, value: f64) {
        self.score_volume = upper_only_volume(value);
    }

    /// Seed native constructor defaults with the screen fields already chosen
    /// by the command-line switch pass.
    ///
    /// A later INI read uses these values as its per-key defaults, so a present
    /// profile key overrides the switch while a missing key leaves it intact.
    pub(crate) fn from_startup(startup: &RetailStartupOptions) -> Self {
        Self {
            screen_width: startup.screen_width,
            screen_height: startup.screen_height,
            ..Default::default()
        }
    }

    /// Apply only the two Video fields read before window creation.
    fn apply_startup_video_ini(&mut self, ini: &IniFile) {
        if let Some(video) = ini.section(VIDEO_SECTION) {
            self.screen_width = video.read_int("ScreenWidth", self.screen_width);
            self.screen_height = video.read_int("ScreenHeight", self.screen_height);
        }
    }

    /// Apply all modeled fields through the shared native typed-reader service.
    /// Missing sections/keys preserve the current field value.
    pub(crate) fn apply_ini(&mut self, ini: &IniFile) {
        // Retail provenance: complete typed profile load and field transforms —
        // `OptionsClass__ReadFromINI` @ `0x005FA620`.
        if let Some(options) = ini.section(OPTIONS_SECTION) {
            self.game_speed = options.read_int("GameSpeed", self.game_speed);
            self.difficulty = options.read_int("Difficulty", self.difficulty).clamp(0, 4);
            self.camp_difficulty = options
                .read_int("CampDifficulty", self.camp_difficulty)
                .clamp(0, 2);
            self.scroll_method = options.read_int("ScrollMethod", self.scroll_method);
            self.scroll_rate = options.read_int("ScrollRate", self.scroll_rate);
            self.auto_scroll = options.read_bool("AutoScroll", self.auto_scroll);
            self.detail_level = options
                .read_int("DetailLevel", self.detail_level)
                .clamp(0, 2);
            self.sidebar_cameo_text =
                options.read_bool("SidebarCameoText", self.sidebar_cameo_text);
            self.unit_action_lines = options.read_bool("UnitActionLines", self.unit_action_lines);
            self.show_hidden = options.read_bool("ShowHidden", self.show_hidden);
            self.tooltips = options.read_bool("ToolTips", self.tooltips);
        }

        if let Some(video) = ini.section(VIDEO_SECTION) {
            self.screen_width = video.read_int("ScreenWidth", self.screen_width);
            self.screen_height = video.read_int("ScreenHeight", self.screen_height);
            // Native additionally ANDs StretchMovies with a legacy capability
            // byte. No equivalent consumer/capability probe exists yet, so the
            // requested value is retained under the design's bounded residual.
            self.stretch_movies = video.read_bool("StretchMovies", self.stretch_movies);
            self.allow_hi_res_modes = video.read_bool("AllowHiResModes", self.allow_hi_res_modes);
            self.allow_mode_toggle = video.read_bool("AllowModeToggle", self.allow_mode_toggle);
            self.allow_vram_sidebar = video.read_bool("AllowVRAMSidebar", self.allow_vram_sidebar);
        }

        if let Some(audio) = ini.section(AUDIO_SECTION) {
            self.sound_volume =
                upper_only_volume(audio.read_double("SoundVolume", f64::from(self.sound_volume)));
            self.voice_volume =
                upper_only_volume(audio.read_double("VoiceVolume", f64::from(self.voice_volume)));
            self.set_score_volume(audio.read_double("ScoreVolume", f64::from(self.score_volume)));
            self.is_score_repeat = audio.read_bool("IsScoreRepeat", self.is_score_repeat);
            self.is_score_shuffle = audio.read_bool("IsScoreShuffle", self.is_score_shuffle);
            self.in_game_music = audio.read_bool("InGameMusic", self.in_game_music);
            // Native performs an ordinary signed ReadInt and stores the low
            // sixteen bits into the profile object after its intervening
            // Network NetID decode. Network semantics stay outside this slice.
            self.sound_latency =
                audio.read_int("SoundLatency", i32::from(self.sound_latency)) as u16;
        }

        self.movie_progress = decode_net_id(ini.section_or_empty(NETWORK_SECTION));
        let network = ini.section_or_empty(NETWORK_SECTION);
        self.network_socket = network.read_int("Socket", i32::from(self.network_socket)) as u16;
        self.network_card = network.read_int("NetCard", self.network_card);
        self.network_destination = network.read_string("DestNet", "", 0x40);
        let multiplayer = ini.section_or_empty("MultiPlayer");
        self.network_send_delay = multiplayer.read_bool("SendDelay", false);
        self.network_port = multiplayer.read_int("PortNumberOverride", 0);
        self.wol_preferences = read_wol_preferences(ini.section_or_empty(WONLINE_SECTION));

        if let Some(vera) = ini.section(VERA_SECTION) {
            self.full_screen = vera.read_bool("FullScreen", self.full_screen);
            self.zoom = crate::ui::main_menu_dialogs::options::Zoom::from_ini_value(
                vera.read_int("Zoom", self.zoom.ini_value()),
            );
            self.original_shape = vera.read_bool("OriginalShape", self.original_shape);
            self.sidebar = crate::ui::main_menu_dialogs::options::SidebarSize::from_ini_value(
                vera.read_int("Sidebar", self.sidebar.ini_value()),
            );
            self.yr_original_style = vera.read_bool("YROriginalStyle", self.yr_original_style);
            self.render_interpolation =
                vera.read_bool("RenderInterpolation", self.render_interpolation);
            self.mouse_wheel_zoom = vera.read_bool("MouseWheelZoom", self.mouse_wheel_zoom);
            self.shift_queue = vera.read_bool("ShiftQueue", self.shift_queue);
            // Absent keeps the current value; present-but-nonnumeric answers 0,
            // which the range lifts to 1 rather than a click that queues nothing.
            self.shift_queue_count = clamp_shift_queue_count(
                vera.read_int("ShiftQueueCount", self.shift_queue_count as i32),
            );
            self.wol_test_service = vera.read_bool("WolTestService", self.wol_test_service);
            self.solo_skirmish = vera.read_bool("SoloSkirmish", self.solo_skirmish);
            if let Some(value) = vera.read_name("TranslateChat", 16) {
                self.translate_chat = crate::ui::chat_translation::ChatLanguage::from_tag(value);
            }
            if let Some(value) = vera.read_name("Display", DISPLAY_CAPACITY) {
                self.display =
                    crate::ui::main_menu_dialogs::options::DisplayTarget::from_ini_value(value);
            }
        }
    }

    /// Mutate the early-read live pair through startup fallback.
    ///
    /// If either field is still the exact `-1` sentinel, retail replaces both
    /// as one pair. The later full read may then reapply a present single key
    /// over this fallback for retained profile state. Explicit non-sentinel
    /// zero/negative values are retained. VERA sizes no window from the pair:
    /// it only round-trips to VERA.INI.
    fn resolve_screen_size(&mut self) {
        // Retail provenance: pre-window screen-pair fallback — `WinMain` @
        // `0x006BB9A0`, branch `0x006BD94A..0x006BD9B5`.
        if self.screen_width == SCREEN_SIZE_UNSET || self.screen_height == SCREEN_SIZE_UNSET {
            self.screen_width = DEFAULT_SCREEN_WIDTH;
            self.screen_height = DEFAULT_SCREEN_HEIGHT;
        }
    }

    /// Format the complete owned snapshot in `WriteToINI` field order.
    pub(crate) fn formatted_sections(&self) -> FormattedProfileSections {
        // Retail provenance: Options/Video/Audio write set, order, and lexical
        // forms — `OptionsClass__WriteToINI` @ `0x005FAD10`, helpers
        // `0x005275C0`, `0x00529560`, and `0x005285B0`.
        FormattedProfileSections {
            options: [
                ("GameSpeed", self.game_speed.to_string()),
                ("Difficulty", self.difficulty.to_string()),
                ("CampDifficulty", self.camp_difficulty.to_string()),
                ("ScrollMethod", self.scroll_method.to_string()),
                ("ScrollRate", self.scroll_rate.to_string()),
                ("AutoScroll", format_bool(self.auto_scroll).to_string()),
                ("DetailLevel", self.detail_level.to_string()),
                (
                    "SidebarCameoText",
                    format_bool(self.sidebar_cameo_text).to_string(),
                ),
                (
                    "UnitActionLines",
                    format_bool(self.unit_action_lines).to_string(),
                ),
                ("ShowHidden", format_bool(self.show_hidden).to_string()),
                ("ToolTips", format_bool(self.tooltips).to_string()),
            ],
            video: [
                ("ScreenWidth", self.screen_width.to_string()),
                ("ScreenHeight", self.screen_height.to_string()),
                (
                    "StretchMovies",
                    format_bool(self.stretch_movies).to_string(),
                ),
            ],
            audio: [
                ("SoundVolume", format!("{:.6}", self.sound_volume)),
                ("VoiceVolume", format!("{:.6}", self.voice_volume)),
                ("ScoreVolume", format!("{:.6}", self.score_volume)),
                (
                    "IsScoreRepeat",
                    format_bool(self.is_score_repeat).to_string(),
                ),
                (
                    "IsScoreShuffle",
                    format_bool(self.is_score_shuffle).to_string(),
                ),
                ("SoundLatency", self.sound_latency.to_string()),
                ("InGameMusic", format_bool(self.in_game_music).to_string()),
            ],
            network: [
                ("Socket", self.network_socket.to_string()),
                ("NetCard", self.network_card.to_string()),
                ("DestNet", self.network_destination.clone()),
                ("NetID", encode_net_id(self.movie_progress)),
            ],
            vera: [
                ("FullScreen", format_bool(self.full_screen).to_string()),
                ("Zoom", self.zoom.ini_value().to_string()),
                (
                    "OriginalShape",
                    format_bool(self.original_shape).to_string(),
                ),
                ("Sidebar", self.sidebar.ini_value().to_string()),
                (
                    "YROriginalStyle",
                    format_bool(self.yr_original_style).to_string(),
                ),
                ("ShiftQueue", format_bool(self.shift_queue).to_string()),
                ("ShiftQueueCount", self.shift_queue_count.to_string()),
                (
                    "WolTestService",
                    format_bool(self.wol_test_service).to_string(),
                ),
                ("SoloSkirmish", format_bool(self.solo_skirmish).to_string()),
                (
                    "RenderInterpolation",
                    format_bool(self.render_interpolation).to_string(),
                ),
                (
                    "MouseWheelZoom",
                    format_bool(self.mouse_wheel_zoom).to_string(),
                ),
                ("Display", self.display.ini_value().to_owned()),
            ],
        }
    }

    /// Transform one raw profile snapshot without performing filesystem I/O.
    pub(crate) fn transform_settings(&self, input: &[u8]) -> Vec<u8> {
        let formatted = self.formatted_sections();
        let output = apply_formatted_section(input, OPTIONS_SECTION, &formatted.options);
        let output = apply_formatted_section(&output, VIDEO_SECTION, &formatted.video);
        let output = apply_formatted_section(&output, AUDIO_SECTION, &formatted.audio);
        // DestNet is the native byte string returned by NewEdit/ReadString.
        // Re-encoding it as UTF-8 would alter high-byte suffixes accepted by %x.
        let network: Vec<_> = formatted
            .network
            .iter()
            .map(|(key, value)| {
                (
                    *key,
                    value
                        .encode_utf16()
                        .map(|unit| unit as u8)
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        let network: Vec<_> = network
            .iter()
            .map(|(key, bytes)| (*key, bytes.as_slice()))
            .collect();
        let output =
            crate::util::ini_writer::set_ini_byte_values(&output, NETWORK_SECTION, &network);
        let output = apply_formatted_section(&output, VERA_SECTION, &formatted.vera);
        crate::util::ini_writer::remove_ini_keys(
            &output,
            VERA_SECTION,
            &["RA2Look", "SoundDefaultsApplied"],
        )
    }

    /// Commit the complete modeled profile with one read and one write.
    ///
    /// `NotFound` is the only read failure that permits creating a new file.
    /// Refusing to replace other unreadable inputs prevents a partial snapshot
    /// from destroying profile bytes owned by other systems.
    pub(crate) fn commit_settings(&self, ra2_dir: &Path) -> io::Result<()> {
        profile_file::update(ra2_dir, |input| self.transform_settings(input))
    }

    /// D7's accepted firewall settings write only these two MultiPlayer keys
    /// (NetworkOptionsDialog__DlgProc_D7 at 560480). LAN fields are written
    /// by the parent Options writer.
    pub(crate) fn commit_network_options(&self, ra2_dir: &Path) -> io::Result<()> {
        profile_file::update(ra2_dir, |input| self.transform_network_options(input))
    }

    /// WOL Options' writer `0x0077DFD0`: the nine `[WOnline]` keys in the
    /// reader's order, `WriteInt` decimal then `WriteBool` yes/no, and then
    /// `INIClass::Save`. Every other section and key is left as it is.
    pub(crate) fn commit_wol_preferences(&self, ra2_dir: &Path) -> io::Result<()> {
        profile_file::update(ra2_dir, |input| self.transform_wol_preferences(input))
    }

    /// The `/translate` command's writer: `[VERA] TranslateChat=` alone.
    pub(crate) fn commit_chat_translation(&self, ra2_dir: &Path) -> io::Result<()> {
        profile_file::update(ra2_dir, |input| self.transform_chat_translation(input))
    }

    fn transform_chat_translation(&self, input: &[u8]) -> Vec<u8> {
        let tag = self.translate_chat.map_or("off", |language| language.tag());
        set_ini_values(input, VERA_SECTION, &[("TranslateChat", tag)])
    }

    fn transform_wol_preferences(&self, input: &[u8]) -> Vec<u8> {
        let p = &self.wol_preferences;
        apply_formatted_section(
            input,
            WONLINE_SECTION,
            &[
                ("AllowPage", p.allow_page.to_string()),
                ("AllowFind", p.allow_find.to_string()),
                ("LangFilter", p.lang_filter.to_string()),
                ("LobMusic", p.lobby_music.to_string()),
                ("ShowAll", p.show_all.to_string()),
                ("DisplayAsian", p.display_asian.to_string()),
                ("DisplayLatin", p.display_latin.to_string()),
                ("DisplayBuddies", format_bool(p.display_buddies).to_owned()),
                ("DisplayClan", format_bool(p.display_clan).to_owned()),
            ],
        )
    }

    fn transform_network_options(&self, input: &[u8]) -> Vec<u8> {
        apply_formatted_section(
            input,
            "MultiPlayer",
            &[
                ("SendDelay", format_bool(self.network_send_delay).to_owned()),
                ("PortNumberOverride", self.network_port.to_string()),
            ],
        )
    }

    /// Injectable form of [`Self::commit_settings`] used to verify the physical
    /// one-read/one-write transaction without touching a user's profile.
    #[cfg(test)]
    pub(crate) fn commit_settings_with<Read, Write>(
        &self,
        ra2_dir: &Path,
        read: Read,
        write: Write,
    ) -> io::Result<()>
    where
        Read: FnOnce(&Path) -> io::Result<Vec<u8>>,
        Write: FnOnce(&Path, &[u8]) -> io::Result<()>,
    {
        profile_file::update_with(ra2_dir, read, write, |input| self.transform_settings(input))
    }
}

/// `0x0077DED0`: ReadInt `0x5276D0` for seven keys and ReadBool `0x5295F0`
/// for two, each with a constant default (not the retained value), no clamps.
fn read_wol_preferences(
    wonline: &crate::rules::ini_parser::IniSection,
) -> crate::ui::wol_options::WolPreferences {
    crate::ui::wol_options::WolPreferences {
        allow_page: wonline.read_int("AllowPage", 1),
        allow_find: wonline.read_int("AllowFind", 1),
        lang_filter: wonline.read_int("LangFilter", 1),
        lobby_music: wonline.read_int("LobMusic", 1),
        show_all: wonline.read_int("ShowAll", 1),
        display_asian: wonline.read_int("DisplayAsian", 1),
        display_latin: wonline.read_int("DisplayLatin", 1),
        display_buddies: wonline.read_bool("DisplayBuddies", false),
        display_clan: wonline.read_bool("DisplayClan", false),
    }
}

fn upper_only_volume(value: f64) -> f32 {
    // An explicit branch matters for NaN: native's ordered comparison is
    // false, so NaN passes through instead of `min` selecting the other input.
    if value >= 1.0 { 1.0 } else { value as f32 }
}

fn format_bool(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

/// Lift a configured `[VERA] ShiftQueueCount=` into the range a click can use.
///
/// `read_int` answers 0 for a present-but-nonnumeric value and a hand-edited
/// file can hold any `i32`, so both ends are clamped: below the minimum a
/// click would queue nothing, above the maximum one press could flood the
/// strip past what the factory would ever accept from one click.
fn clamp_shift_queue_count(value: i32) -> u32 {
    value.clamp(
        *SHIFT_QUEUE_COUNT_RANGE.start() as i32,
        *SHIFT_QUEUE_COUNT_RANGE.end() as i32,
    ) as u32
}

fn apply_formatted_section<const N: usize>(
    input: &[u8],
    section: &str,
    values: &[(&'static str, String); N],
) -> Vec<u8> {
    let borrowed: Vec<(&str, &str)> = values
        .iter()
        .map(|(key, value)| (*key, value.as_str()))
        .collect();
    set_ini_values(input, section, &borrowed)
}

/// The NetID obfuscation: every UTF-16 unit is bitwise-NOT'd, and a unit
/// that becomes zero is stored as `0xFFFF` so the string keeps its length
/// (`0x005FABD2..0x005FABEC`, `0x005FAFC6..0x005FAFE0`).
fn flip_net_id_units(units: &[u16]) -> Vec<u16> {
    units
        .iter()
        .map(|unit| match !unit {
            0 => 0xFFFF,
            flipped => flipped,
        })
        .collect()
}

/// CRT `wcstol(token, NULL, 10)`: leading wide whitespace, optional sign,
/// decimal digits; no digits yields 0 and overflow saturates.
fn wcstol_decimal(token: &[u16]) -> i32 {
    let is_space = |unit: u16| matches!(unit, 0x09..=0x0D | 0x20);
    let mut rest = token;
    while let Some((&first, tail)) = rest.split_first() {
        if !is_space(first) {
            break;
        }
        rest = tail;
    }
    let negative = rest.first() == Some(&u16::from(b'-'));
    if matches!(rest.first(), Some(&unit) if unit == u16::from(b'-') || unit == u16::from(b'+')) {
        rest = &rest[1..];
    }
    let mut value: i64 = 0;
    for &unit in rest {
        if !(u16::from(b'0')..=u16::from(b'9')).contains(&unit) {
            break;
        }
        value = value * 10 + i64::from(unit - u16::from(b'0'));
        if value > i64::from(i32::MAX) + 1 {
            break;
        }
    }
    let value = if negative { -value } else { value };
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// Decode `[Network] NetID` into movie progress. Both fields start at -1, the
/// first two `wcstok(L" ")` tokens are read with `wcstol` minus one, and a
/// value outside -1..=7 falls back to -1.
fn decode_net_id(network: &IniSection) -> crate::ui::movies_credits_shell::MovieProgress {
    let units = network.read_comma_hex_utf16("NetID", &[], NET_ID_UNITS);
    let text = flip_net_id_units(&units);
    let mut tokens = text
        .split(|unit| *unit == u16::from(b' '))
        .filter(|token| !token.is_empty());
    let mut progress = crate::ui::movies_credits_shell::MovieProgress::default();
    if let Some(token) = tokens.next() {
        progress.soviet = wcstol_decimal(token).wrapping_sub(1);
        if let Some(token) = tokens.next() {
            progress.allied = wcstol_decimal(token).wrapping_sub(1);
        }
    }
    let clamp = |value: i32| if (-1..8).contains(&value) { value } else { -1 };
    progress.soviet = clamp(progress.soviet);
    progress.allied = clamp(progress.allied);
    progress
}

/// Encode movie progress the way `WriteToINI` does: `swprintf(L"%d %d",
/// soviet + 1, allied + 1)`, flipped, then written as comma hex.
fn encode_net_id(progress: crate::ui::movies_credits_shell::MovieProgress) -> String {
    let text = format!(
        "{} {}",
        progress.soviet.wrapping_add(1),
        progress.allied.wrapping_add(1)
    );
    let units: Vec<u16> = text.encode_utf16().collect();
    crate::rules::ini_value::encode_comma_hex_utf16(&flip_net_id_units(&units))
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use super::*;

    #[test]
    fn network_settings_read_native_defaults_and_narrow_socket_only() {
        let defaults = load_snapshot(&RetailStartupOptions::default(), b"").retained_profile;
        assert_eq!(
            (
                defaults.network_socket,
                defaults.network_card,
                defaults.network_port
            ),
            (65535, 0, 0)
        );
        assert!(!defaults.network_send_delay);
        assert!(defaults.network_destination.is_empty());
        let loaded = load_snapshot(&RetailStartupOptions::default(), b"[Network]\nSocket=65536\nNetCard=-2\nDestNet=  aa.bb.cc.dd  \n[MultiPlayer]\nSendDelay=yes\nPortNumberOverride=-1\n").retained_profile;
        assert_eq!(
            (
                loaded.network_socket,
                loaded.network_card,
                loaded.network_port
            ),
            (0, -2, -1)
        );
        assert!(loaded.network_send_delay);
        assert_eq!(loaded.network_destination, "aa.bb.cc.dd");
    }

    #[test]
    fn d7_commit_keeps_lan_pending_until_parent_options_write() {
        let input = b"; keep\r\n[Network]\r\nSocket=5\r\nNetCard=1\r\nDestNet=1.2.3.4\r\n[MultiPlayer]\r\nHandle=unchanged\r\nSendDelay=no\r\nPortNumberOverride=0\r\n";
        let profile = RetailOptionsProfile {
            network_socket: 123,
            network_card: 2,
            network_destination: "aa.bb.cc.dd".into(),
            network_send_delay: true,
            network_port: -1,
            ..Default::default()
        };
        let d7 = profile.transform_network_options(input);
        let parsed = IniFile::from_bytes(&d7).unwrap();
        assert_eq!(parsed.section_or_empty("Network").read_int("Socket", 0), 5);
        assert_eq!(
            parsed
                .section_or_empty("MultiPlayer")
                .read_string("Handle", "", 64),
            "unchanged"
        );
        assert!(
            parsed
                .section_or_empty("MultiPlayer")
                .read_bool("SendDelay", false)
        );
        assert_eq!(
            parsed
                .section_or_empty("MultiPlayer")
                .read_int("PortNumberOverride", 0),
            -1
        );
        let parent = profile.transform_settings(&d7);
        let parsed = IniFile::from_bytes(&parent).unwrap();
        assert_eq!(
            parsed.section_or_empty("Network").read_int("Socket", 0),
            123
        );
        assert_eq!(
            parsed
                .section_or_empty("Network")
                .read_string("DestNet", "", 64),
            "aa.bb.cc.dd"
        );
        assert_eq!(
            parsed
                .section_or_empty("MultiPlayer")
                .read_int("PortNumberOverride", 0),
            -1
        );
    }

    #[test]
    fn destination_round_trips_native_bytes_without_utf8_encoding() {
        let loaded = load_snapshot(
            &RetailStartupOptions::default(),
            b"[Network]\nDestNet=1\xe9.2.3.4\n",
        )
        .retained_profile;
        let output = loaded.transform_settings(b"[Network]\nDestNet=old\n");
        let reloaded = load_snapshot(&RetailStartupOptions::default(), &output).retained_profile;
        assert_eq!(loaded.network_destination, "1é.2.3.4");
        assert_eq!(reloaded.network_destination, loaded.network_destination);
    }

    /// `CRCEngine::operator()` @ `0x004A1DE0` over `"Network"`, executed
    /// natively under Unicorn: the scratch `INIClass__ReadCommaHexUTF16`
    /// leaves for a failed first `%x` conversion on the fresh-file cache miss.
    #[test]
    fn network_section_crc_is_the_crc_engine_of_its_name() {
        assert_eq!(
            crate::assets::mix_hash::crc_engine(NETWORK_SECTION.as_bytes()),
            0x70CA_A741
        );
    }

    fn load_snapshot(startup: &RetailStartupOptions, bytes: &[u8]) -> RetailOptionsLoad {
        RetailOptionsLoad::from_settings_with(Path::new("C:/retail"), startup, |_| {
            Ok(bytes.to_vec())
        })
    }

    #[test]
    fn profile_defaults_match_optionsclass() {
        let profile = RetailOptionsProfile::default();
        assert_eq!(profile.game_speed, 3);
        assert_eq!(profile.difficulty, 1);
        assert_eq!(profile.camp_difficulty, 0);
        assert_eq!(profile.scroll_method, 0);
        assert_eq!(profile.scroll_rate, 3);
        assert!(profile.auto_scroll);
        assert_eq!(profile.detail_level, 2);
        assert!(profile.sidebar_cameo_text);
        assert!(profile.unit_action_lines);
        assert!(!profile.show_hidden);
        assert!(profile.tooltips);
        assert_eq!(profile.screen_width, -1);
        assert_eq!(profile.screen_height, -1);
        assert!(!profile.stretch_movies);
        assert!(!profile.allow_hi_res_modes);
        assert!(!profile.allow_mode_toggle);
        assert!(!profile.allow_vram_sidebar);
        assert_eq!(profile.sound_volume.to_bits(), 0.7_f32.to_bits());
        assert_eq!(profile.voice_volume.to_bits(), 0.7_f32.to_bits());
        assert_eq!(profile.score_volume.to_bits(), 0.4_f32.to_bits());
        assert!(!profile.is_score_repeat);
        assert!(!profile.is_score_shuffle);
        assert_eq!(profile.sound_latency, 9);
        assert!(profile.in_game_music);
    }

    /// Executed Options5FA4A0 controls retain the input/store f32 bits,
    /// including upper clamp, signed zero, negative and nonfinite values.
    #[test]
    fn score_volume_store_matches_native_options_setter() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/sound_theme_metadata.json",
        ))
        .unwrap();
        let cases = corpus["score_volume_cases"].as_array().unwrap();
        assert_eq!(cases.len(), 16);
        for case in cases {
            let input = f32::from_bits(case["input_f32_bits"].as_u64().unwrap() as u32);
            let mut profile = RetailOptionsProfile::default();
            profile.set_score_volume(f64::from(input));
            assert_eq!(
                profile.score_volume.to_bits(),
                case["stored_f32_bits"].as_u64().unwrap() as u32,
                "{case}"
            );
        }
    }

    #[test]
    fn wol_preferences_read_with_constant_defaults_and_write_in_native_order() {
        use crate::ui::wol_options::WolPreferences;
        let mut profile = RetailOptionsProfile::default();
        profile.apply_ini(&IniFile::from_str("[WOnline]\nNick=x\n"));
        assert_eq!(profile.wol_preferences, WolPreferences::default());
        profile.apply_ini(&IniFile::from_str(
            "[WOnline]\nLobMusic=$2\nAllowPage=0\nDisplayClan=yes\n",
        ));
        assert_eq!(
            profile.wol_preferences,
            WolPreferences {
                allow_page: 0,
                lobby_music: 2,
                display_clan: true,
                ..WolPreferences::default()
            },
            "ReadInt takes $ hex; ReadBool takes yes"
        );
        // An absent key reads its constant default again, not the retained value.
        profile.apply_ini(&IniFile::from_str("[WOnline]\n"));
        assert_eq!(profile.wol_preferences, WolPreferences::default());

        profile.wol_preferences = WolPreferences {
            allow_page: 0,
            display_buddies: true,
            ..WolPreferences::default()
        };
        let written = profile.transform_wol_preferences(
            b"[WOnline]\r\nNick=x\r\nLobMusic=$2\r\n[MultiPlayer]\r\nObserverMode=1\r\n",
        );
        let ini = IniFile::from_bytes(&written).unwrap();
        let wonline = ini.section("WOnline").unwrap();
        assert_eq!(wonline.get_for_test("Nick"), Some("x"));
        assert_eq!(wonline.get_for_test("AllowPage"), Some("0"));
        assert_eq!(wonline.get_for_test("LobMusic"), Some("1"));
        assert_eq!(wonline.get_for_test("DisplayBuddies"), Some("yes"));
        assert_eq!(wonline.get_for_test("DisplayClan"), Some("no"));
        assert_eq!(
            ini.section("MultiPlayer")
                .unwrap()
                .get_for_test("ObserverMode"),
            Some("1")
        );
        // New keys append in the writer's order.
        let fresh = profile.transform_wol_preferences(b"[WOnline]\r\nNick=x\r\n");
        let text = String::from_utf8_lossy(&fresh);
        let order: Vec<_> = [
            "Nick",
            "AllowPage",
            "AllowFind",
            "LangFilter",
            "LobMusic",
            "ShowAll",
            "DisplayAsian",
            "DisplayLatin",
            "DisplayBuddies",
            "DisplayClan",
        ]
        .iter()
        .map(|key| text.find(&format!("{key}=")).unwrap())
        .collect();
        assert!(order.windows(2).all(|pair| pair[0] < pair[1]), "{text}");
    }

    #[test]
    fn profile_load_uses_native_typed_semantics_and_clamps() {
        let ini = IniFile::from_str(
            "[Options]\n\
             GameSpeed=not-a-number\n\
             Difficulty=99\n\
             CampDifficulty=-7\n\
             ScrollMethod=$2A\n\
             ScrollRate=-19\n\
             AutoScroll=maybe\n\
             DetailLevel=9\n\
             SidebarCameoText=Nope\n\
             UnitActionLines=Yikes\n\
             ShowHidden=1anything\n\
             ToolTips=0anything\n\
             [Video]\n\
             ScreenWidth=640\n\
             ScreenHeight=480\n\
             StretchMovies=TRUE\n\
             AllowHiResModes=YES\n\
             AllowModeToggle=1\n\
             AllowVRAMSidebar=No\n\
             [Audio]\n\
             SoundVolume=-0.250000\n\
             VoiceVolume=75% trailing\n\
             ScoreVolume=4.0\n\
             IsScoreRepeat=TRUE\n\
             IsScoreShuffle=No\n\
             SoundLatency=65537\n\
             InGameMusic=FALSE\n",
        );
        let mut profile = RetailOptionsProfile::default();
        profile.apply_ini(&ini);

        assert_eq!(profile.game_speed, 0, "present malformed decimal uses atoi");
        assert_eq!(profile.difficulty, 4);
        assert_eq!(profile.camp_difficulty, 0);
        assert_eq!(profile.scroll_method, 0x2a);
        assert_eq!(profile.scroll_rate, -19, "unbounded Options integer");
        assert!(
            profile.auto_scroll,
            "invalid bool preserves current default"
        );
        assert_eq!(profile.detail_level, 2);
        assert!(!profile.sidebar_cameo_text);
        assert!(profile.unit_action_lines);
        assert!(profile.show_hidden);
        assert!(!profile.tooltips);
        assert_eq!((profile.screen_width, profile.screen_height), (640, 480));
        assert!(profile.stretch_movies);
        assert!(profile.allow_hi_res_modes);
        assert!(profile.allow_mode_toggle);
        assert!(!profile.allow_vram_sidebar);
        assert_eq!(
            profile.sound_volume, -0.25,
            "negative profile gain survives"
        );
        let expected_percent = (f64::from(75.0_f32) * 0.01) as f32;
        assert_eq!(profile.voice_volume.to_bits(), expected_percent.to_bits());
        assert_eq!(profile.score_volume, 1.0, "upper clamp only");
        assert!(profile.is_score_repeat);
        assert!(!profile.is_score_shuffle);
        assert_eq!(profile.sound_latency, 1, "native low-16-bit narrowing");
        assert!(!profile.in_game_music);
    }

    #[test]
    fn missing_keys_preserve_current_fields() {
        let mut profile = RetailOptionsProfile {
            scroll_rate: 123,
            voice_volume: -0.375,
            allow_vram_sidebar: true,
            ..RetailOptionsProfile::default()
        };
        profile.apply_ini(&IniFile::from_str(
            "[Options]\nGameSpeed=5\n[Audio]\nSoundVolume=0.25\n",
        ));
        assert_eq!(profile.scroll_rate, 123);
        assert_eq!(profile.voice_volume, -0.375);
        assert!(profile.allow_vram_sidebar);
    }

    #[test]
    fn first_start_preserves_imported_audio_and_keeps_saved_choices() {
        let audio = |p: &RetailOptionsProfile| {
            (
                p.sound_volume,
                p.voice_volume,
                p.score_volume,
                p.is_score_repeat,
                p.is_score_shuffle,
                p.sound_latency,
                p.in_game_music,
            )
        };
        let startup = RetailStartupOptions::default();
        // A profile the original game or an older VERA20k wrote.
        let old = b"[Options]\nGameSpeed=5\n[Audio]\nSoundVolume=0.5\nVoiceVolume=0.6\n\
ScoreVolume=0.3\nIsScoreShuffle=yes\nSoundLatency=4\nInGameMusic=no\n[VERA]\nZoom=150\n";
        let first = load_snapshot(&startup, old).retained_profile;
        assert_eq!(first.sound_volume, 0.5);
        assert_eq!(first.voice_volume, 0.6);
        assert_eq!(first.score_volume, 0.3);
        assert!(first.is_score_shuffle);
        assert!(!first.in_game_music);
        assert_eq!(first.game_speed, 5);

        // Any save records it, so the player's choices survive restarts.
        let chosen = RetailOptionsProfile {
            sound_volume: 0.25,
            ..first
        };
        let saved = chosen.transform_settings(old);
        let second = load_snapshot(&startup, &saved).retained_profile;
        assert_eq!(audio(&second), audio(&chosen));
    }

    #[test]
    fn width_only_snapshot_uses_fallback_window_then_rereads_retained_width() {
        let reads = Cell::new(0);
        let load = RetailOptionsLoad::from_settings_with(
            Path::new("C:/retail"),
            &RetailStartupOptions::default(),
            |path| {
                reads.set(reads.get() + 1);
                assert_eq!(path, Path::new("C:/retail").join(profile_file::FILENAME));
                Ok(b"[Options]\nGameSpeed=5\n[Video]\nScreenWidth=640\n".to_vec())
            },
        );

        assert_eq!(reads.get(), 1, "both semantic reads share one snapshot");
        assert_eq!(
            load.startup_shell_screen,
            ScreenSize {
                width: 800,
                height: 600,
            }
        );
        assert_eq!(
            (
                load.retained_profile.screen_width,
                load.retained_profile.screen_height,
            ),
            (640, 600)
        );
        assert_eq!(
            load.retained_profile.game_speed, 5,
            "the later pass still loads non-Video profile fields"
        );
    }

    #[test]
    fn ordinary_frontend_size_is_independent_of_game_profile_and_launcher_edits() {
        for (width, height) in [(640, 480), (800, 600), (1024, 768)] {
            let ini = format!("[Video]\nScreenWidth={width}\nScreenHeight={height}\n");
            let load = load_snapshot(&RetailStartupOptions::default(), ini.as_bytes());
            assert_eq!(load.startup_shell_screen, RETAIL_SHELL_SIZE);
            assert_eq!(
                (
                    load.retained_profile.screen_width,
                    load.retained_profile.screen_height
                ),
                (width, height)
            );
        }
    }

    #[test]
    fn explicit_mode_toggle_selects_low_startup_without_replacing_game_preference() {
        let load = load_snapshot(
            &RetailStartupOptions::default(),
            b"[Video]\nAllowModeToggle=yes\nScreenWidth=1024\nScreenHeight=768\n",
        );
        assert_eq!(
            load.startup_shell_screen,
            ScreenSize {
                width: 640,
                height: 480
            }
        );
        assert_eq!(
            (
                load.retained_profile.screen_width,
                load.retained_profile.screen_height
            ),
            (1024, 768)
        );
    }

    #[test]
    fn present_ini_key_overrides_argv_while_missing_key_keeps_argv_default() {
        let startup = RetailStartupOptions {
            screen_width: 1024,
            screen_height: 768,
            ..RetailStartupOptions::default()
        };
        let load = load_snapshot(&startup, b"[Video]\nScreenWidth=640\n");

        assert_eq!(
            load.startup_shell_screen,
            ScreenSize {
                width: 800,
                height: 600,
            }
        );
        assert_eq!(
            (
                load.retained_profile.screen_width,
                load.retained_profile.screen_height,
            ),
            (640, 768)
        );

        let missing_both = load_snapshot(&startup, b"[Options]\nGameSpeed=4\n");
        assert_eq!(
            missing_both.startup_shell_screen,
            ScreenSize {
                width: 800,
                height: 600,
            }
        );
        assert_eq!(
            (
                missing_both.retained_profile.screen_width,
                missing_both.retained_profile.screen_height,
            ),
            (1024, 768)
        );
    }

    #[test]
    fn no_snapshot_paths_preserve_complete_argv_and_fallback_partial_argv() {
        let complete = RetailStartupOptions {
            screen_width: 1024,
            screen_height: 768,
            ..RetailStartupOptions::default()
        };
        let missing =
            RetailOptionsLoad::from_settings_with(Path::new("C:/retail"), &complete, |_| {
                Err(io::Error::new(io::ErrorKind::NotFound, "missing"))
            });
        assert_eq!(
            missing.startup_shell_screen,
            ScreenSize {
                width: 800,
                height: 600,
            }
        );
        assert_eq!(
            (
                missing.retained_profile.screen_width,
                missing.retained_profile.screen_height,
            ),
            (1024, 768)
        );

        let partial = RetailStartupOptions {
            screen_width: 1280,
            screen_height: SCREEN_SIZE_UNSET,
            ..RetailStartupOptions::default()
        };
        let partial = RetailOptionsLoad::without_settings(&partial);
        assert_eq!(
            partial.startup_shell_screen,
            ScreenSize {
                width: 800,
                height: 600,
            }
        );
        assert_eq!(
            (
                partial.retained_profile.screen_width,
                partial.retained_profile.screen_height,
            ),
            (800, 600)
        );
    }

    #[test]
    fn malformed_screen_integer_keeps_native_typed_reader_behavior() {
        let load = load_snapshot(
            &RetailStartupOptions::default(),
            b"[Video]\nScreenWidth=not-a-number\nScreenHeight=480\n",
        );

        assert_eq!(
            load.startup_shell_screen,
            ScreenSize {
                width: 800,
                height: 600,
            }
        );
        assert_eq!(
            (
                load.retained_profile.screen_width,
                load.retained_profile.screen_height,
            ),
            (0, 480)
        );
    }

    #[test]
    fn vera_display_preferences_round_trip_in_their_own_section() {
        use crate::ui::main_menu_dialogs::options::{SidebarSize, Zoom};
        let defaults = load_snapshot(
            &RetailStartupOptions::default(),
            b"[Video]\nScreenWidth=800\n",
        );
        assert!(!defaults.retained_profile.full_screen);
        assert_eq!(defaults.retained_profile.zoom, Zoom::Auto);
        assert!(!defaults.retained_profile.original_shape);
        assert_eq!(defaults.retained_profile.sidebar, SidebarSize::Normal);
        assert!(!defaults.retained_profile.yr_original_style);
        // Shift + left click queues one copy, as in gamemd, until it is ticked.
        assert!(!defaults.retained_profile.shift_queue);
        // Internet shows TXT_APIMISSING, as on retail, until it is ticked.
        assert!(!defaults.retained_profile.wol_test_service);
        // The retail minimum-player gate stays until SoloSkirmish= is enabled.
        assert!(!defaults.retained_profile.solo_skirmish);
        assert_eq!(
            defaults.retained_profile.shift_queue_count,
            DEFAULT_SHIFT_QUEUE_COUNT
        );

        let load = load_snapshot(
            &RetailStartupOptions::default(),
            b"[VERA]\nFullScreen=yes\nZoom=2\nOriginalShape=yes\nSidebar=150\nShiftQueue=yes\nShiftQueueCount=8\nWolTestService=yes\nSoloSkirmish=yes\n",
        );
        assert!(load.retained_profile.full_screen);
        // The first release saved the whole factor.
        assert_eq!(load.retained_profile.zoom, Zoom::Percent(200));
        assert!(load.retained_profile.original_shape);
        assert_eq!(load.retained_profile.sidebar, SidebarSize::Large);
        assert!(!load.retained_profile.yr_original_style);
        assert!(load.retained_profile.shift_queue);
        assert!(load.retained_profile.wol_test_service);
        assert!(load.retained_profile.solo_skirmish);
        assert_eq!(load.retained_profile.shift_queue_count, 8);
        // A value this build doesn't know reads as Auto.
        let unknown = load_snapshot(&RetailStartupOptions::default(), b"[VERA]\nZoom=9\n");
        assert_eq!(unknown.retained_profile.zoom, Zoom::Auto);

        let profile = RetailOptionsProfile {
            full_screen: true,
            zoom: Zoom::Percent(150),
            original_shape: true,
            sidebar: SidebarSize::ExtraLarge,
            yr_original_style: true,
            shift_queue: true,
            shift_queue_count: 7,
            wol_test_service: true,
            solo_skirmish: true,
            ..RetailOptionsProfile::default()
        };
        let output =
            String::from_utf8(profile.transform_settings(b"[Video]\r\nScreenWidth=800\r\n"))
                .unwrap();
        assert!(
            output.contains(
                "[VERA]\r\nFullScreen=yes\r\nZoom=150\r\nOriginalShape=yes\r\nSidebar=200\r\nYROriginalStyle=yes\r\nShiftQueue=yes\r\nShiftQueueCount=7\r\nWolTestService=yes\r\nSoloSkirmish=yes\r\n"
            ),
            "{output}"
        );
        let reread = load_snapshot(&RetailStartupOptions::default(), output.as_bytes());
        assert!(reread.retained_profile.full_screen);
        assert_eq!(reread.retained_profile.zoom, Zoom::Percent(150));
        assert!(reread.retained_profile.original_shape);
        assert_eq!(reread.retained_profile.sidebar, SidebarSize::ExtraLarge);
        assert!(reread.retained_profile.yr_original_style);
        assert!(reread.retained_profile.shift_queue);
        assert!(reread.retained_profile.wol_test_service);
        assert!(reread.retained_profile.solo_skirmish);
        assert_eq!(reread.retained_profile.shift_queue_count, 7);
    }

    #[test]
    fn chat_translation_writes_only_its_own_key_and_reads_back() {
        use crate::ui::chat_translation::ChatLanguage;
        let startup = RetailStartupOptions::default();
        let profile = RetailOptionsProfile {
            translate_chat: Some(ChatLanguage::SimplifiedChinese),
            ..RetailOptionsProfile::default()
        };
        let output = profile.transform_chat_translation(b"[VERA]\r\nZoom=150\r\n");
        let text = String::from_utf8(output.clone()).unwrap();
        assert!(
            text.contains("Zoom=150") && text.contains("TranslateChat=zh-Hans"),
            "{text}"
        );
        let reread = load_snapshot(&startup, &output);
        assert_eq!(
            reread.retained_profile.translate_chat,
            Some(ChatLanguage::SimplifiedChinese)
        );
        let off = RetailOptionsProfile::default().transform_chat_translation(&output);
        assert_eq!(
            load_snapshot(&startup, &off)
                .retained_profile
                .translate_chat,
            None
        );
    }

    #[test]
    fn display_preference_round_trips_through_the_vera_section() {
        use crate::ui::main_menu_dialogs::options::DisplayTarget;
        let startup = RetailStartupOptions::default();
        // Absent and empty both mean Auto: the system chooses, as every
        // earlier build did.
        for input in [
            b"".as_slice(),
            b"[VERA]\n".as_slice(),
            b"[VERA]\nDisplay=\n".as_slice(),
        ] {
            assert_eq!(
                load_snapshot(&startup, input).retained_profile.display,
                DisplayTarget::Auto
            );
        }
        // A named screen survives a restart even when it is unplugged now; the
        // window layer falls back at use, never at read.
        let named = load_snapshot(&startup, b"[VERA]\nDisplay=\\\\.\\DISPLAY2\n");
        assert_eq!(
            named.retained_profile.display,
            DisplayTarget::Fixed("\\\\.\\DISPLAY2".to_owned())
        );
        // The writer emits the key in the VERA block, and a re-read agrees.
        let profile = RetailOptionsProfile {
            display: DisplayTarget::Fixed("\\\\.\\DISPLAY1".to_owned()),
            ..RetailOptionsProfile::default()
        };
        let output = profile.transform_settings(b"[Video]\r\nScreenWidth=800\r\n");
        assert!(
            String::from_utf8_lossy(&output).contains("Display=\\\\.\\DISPLAY1\r\n"),
            "{}",
            String::from_utf8_lossy(&output)
        );
        assert_eq!(
            load_snapshot(&startup, &output).retained_profile.display,
            DisplayTarget::Fixed("\\\\.\\DISPLAY1".to_owned())
        );
        // Auto writes an empty value, which reads back as Auto.
        let auto = RetailOptionsProfile::default()
            .transform_settings(b"[VERA]\nDisplay=\\\\.\\DISPLAY1\n");
        assert_eq!(
            load_snapshot(&startup, &auto).retained_profile.display,
            DisplayTarget::Auto
        );
    }

    #[test]
    fn mouse_wheel_zoom_preference_round_trips_in_the_existing_vera_section() {
        let startup = RetailStartupOptions::default();
        assert!(
            !load_snapshot(&startup, b"")
                .retained_profile
                .mouse_wheel_zoom
        );
        let input = b"[VERA]\nMouseWheelZoom=yes\nRenderInterpolation=no\nUnowned=keep\n";
        let mut profile = load_snapshot(&startup, input).retained_profile;
        assert!(profile.mouse_wheel_zoom);
        let written = profile.transform_settings(input);
        assert!(
            load_snapshot(&startup, &written)
                .retained_profile
                .mouse_wheel_zoom
        );
        profile.mouse_wheel_zoom = false;
        let written = profile.transform_settings(&written);
        let reloaded = load_snapshot(&startup, &written).retained_profile;
        assert!(!reloaded.mouse_wheel_zoom);
        assert!(!reloaded.render_interpolation);
        assert!(String::from_utf8_lossy(&written).contains("Unowned=keep"));
    }

    #[test]
    fn render_interpolation_preference_uses_the_existing_vera_profile_transaction() {
        let startup = RetailStartupOptions::default();
        assert!(
            !load_snapshot(&startup, b"")
                .retained_profile
                .render_interpolation
        );
        for (value, expected) in [("yes", true), ("no", false)] {
            let input = format!("[VERA]\nRenderInterpolation={value}\nUnowned=keep\n");
            let profile = load_snapshot(&startup, input.as_bytes()).retained_profile;
            assert_eq!(profile.render_interpolation, expected);
            let written = profile.transform_settings(input.as_bytes());
            assert!(String::from_utf8_lossy(&written).contains("Unowned=keep"));
            assert_eq!(
                load_snapshot(&startup, &written)
                    .retained_profile
                    .render_interpolation,
                expected
            );
        }
    }

    #[test]
    fn original_yr_style_and_render_interpolation_round_trip_independently() {
        let startup = RetailStartupOptions::default();
        for style in [false, true] {
            for interpolation in [false, true] {
                let input = format!(
                    "[VERA]\nYROriginalStyle={}\nRenderInterpolation={}\nRA2Look=yes\nUnowned=keep\n",
                    format_bool(style),
                    format_bool(interpolation),
                );
                let profile = load_snapshot(&startup, input.as_bytes()).retained_profile;
                assert_eq!(profile.yr_original_style, style);
                assert_eq!(profile.render_interpolation, interpolation);
                let written = profile.transform_settings(input.as_bytes());
                let text = String::from_utf8_lossy(&written);
                assert!(!text.contains("RA2Look="));
                assert!(text.contains("Unowned=keep"));
                let reloaded = load_snapshot(&startup, &written).retained_profile;
                assert_eq!(reloaded.yr_original_style, style);
                assert_eq!(reloaded.render_interpolation, interpolation);
            }
        }
    }

    #[test]
    fn yr_original_style_defaults_to_ra2_and_reads_only_its_own_key() {
        let defaults = load_snapshot(&RetailStartupOptions::default(), b"[VERA]\n");
        assert!(
            !defaults.retained_profile.yr_original_style,
            "unchecked YR Loading & Menu Movie selects the default RA2 look"
        );

        for value in ["yes", "no"] {
            let ignored = load_snapshot(
                &RetailStartupOptions::default(),
                format!("[VERA]\nRA2Look={value}\n").as_bytes(),
            );
            assert!(
                !ignored.retained_profile.yr_original_style,
                "RA2Look={value} must not affect the default"
            );
        }

        let explicit_new_key = load_snapshot(
            &RetailStartupOptions::default(),
            b"[VERA]\nYROriginalStyle=yes\n",
        );
        assert!(
            explicit_new_key.retained_profile.yr_original_style,
            "the YR-original preference must be read from its own key"
        );
    }

    #[test]
    fn saving_removes_ra2_look_and_preserves_other_profile_bytes() {
        let profile = RetailOptionsProfile {
            yr_original_style: true,
            ..RetailOptionsProfile::default()
        };
        let input = b"[VERA]\r\n; retain this comment\r\nFullScreen=yes\r\nRA2Look=no ; remove this whole line\r\nUnknownSetting=keep\r\n[other]\nRA2Look=keep in another section\n[vera]\nra2look=yes\nShiftQueue=yes";
        let output = String::from_utf8(profile.transform_settings(input)).unwrap();

        assert_eq!(output.matches("RA2Look=").count(), 1, "{output}");
        assert!(!output.contains("ra2look="), "{output}");
        assert!(
            output.contains("YROriginalStyle=yes\r\n"),
            "new preference should be saved: {output}"
        );
        assert!(
            output.contains("; retain this comment\r\nFullScreen=no\r\nUnknownSetting=keep\r\n"),
            "keep the comment and unknown key while the modeled setting uses the profile: {output}"
        );
        assert!(
            output.contains("[other]\nRA2Look=keep in another section\n"),
            "only the VERA key should be removed: {output}"
        );
    }

    /// `read_int` answers 0 for a present-but-nonnumeric value and a hand-edited
    /// file can hold any `i32`; both ends are clamped so a click always sends a
    /// PRODUCE and never floods past what one press should reach.
    #[test]
    fn shift_queue_count_is_clamped_at_both_ends() {
        for (ini, expected) in [
            ("[VERA]\nShiftQueueCount=0\n", 1),
            ("[VERA]\nShiftQueueCount=-4\n", 1),
            ("[VERA]\nShiftQueueCount=nonsense\n", 1),
            ("[VERA]\nShiftQueueCount=1\n", 1),
            ("[VERA]\nShiftQueueCount=30\n", 30),
            ("[VERA]\nShiftQueueCount=31\n", 30),
            ("[VERA]\nShiftQueueCount=99999\n", 30),
        ] {
            let load = load_snapshot(&RetailStartupOptions::default(), ini.as_bytes());
            assert_eq!(load.retained_profile.shift_queue_count, expected, "{ini:?}");
        }
        // An absent key keeps the current value, not the clamp floor.
        let absent = load_snapshot(
            &RetailStartupOptions::default(),
            b"[VERA]\nFullScreen=yes\n",
        );
        assert_eq!(
            absent.retained_profile.shift_queue_count,
            DEFAULT_SHIFT_QUEUE_COUNT
        );
    }

    #[test]
    fn formatted_sections_have_native_order_and_lexical_shape() {
        let profile = RetailOptionsProfile {
            auto_scroll: true,
            tooltips: false,
            voice_volume: 0.7,
            ..RetailOptionsProfile::default()
        };
        let formatted = profile.formatted_sections();
        assert_eq!(
            formatted
                .options
                .iter()
                .map(|pair| pair.0)
                .collect::<Vec<_>>(),
            vec![
                "GameSpeed",
                "Difficulty",
                "CampDifficulty",
                "ScrollMethod",
                "ScrollRate",
                "AutoScroll",
                "DetailLevel",
                "SidebarCameoText",
                "UnitActionLines",
                "ShowHidden",
                "ToolTips",
            ]
        );
        assert_eq!(
            formatted
                .video
                .iter()
                .map(|pair| pair.0)
                .collect::<Vec<_>>(),
            vec!["ScreenWidth", "ScreenHeight", "StretchMovies"]
        );
        assert_eq!(
            formatted
                .audio
                .iter()
                .map(|pair| pair.0)
                .collect::<Vec<_>>(),
            vec![
                "SoundVolume",
                "VoiceVolume",
                "ScoreVolume",
                "IsScoreRepeat",
                "IsScoreShuffle",
                "SoundLatency",
                "InGameMusic",
            ]
        );
        assert_eq!(formatted.options[5].1, "yes");
        assert_eq!(formatted.options[10].1, "no");
        assert_eq!(formatted.audio[1].1, "0.700000");
        assert!(
            formatted
                .video
                .iter()
                .all(|(key, _)| !key.starts_with("Allow"))
        );
    }

    #[test]
    fn profile_commit_is_one_write_and_preserves_unowned_bytes() {
        let input = b"; retail profile\r\n\
[Options]\r\n\
GameSpeed=0\r\nDifficulty=1\r\nCampDifficulty=0\r\nScrollMethod=0\r\n\
ScrollRate=4\r\nAutoScroll=yes\r\nDetailLevel=2\r\nSidebarCameoText=yes\r\n\
UnitActionLines=yes\r\nShowHidden=no\r\nToolTips=yes\r\nUnknownOption=keep\r\n\
[Video]\r\nScreenWidth=640\r\nScreenHeight=480\r\nStretchMovies=no\r\n\
[Audio]\r\nSoundVolume=0.700000\r\nVoiceVolume=0.800000\r\nScoreVolume=0.600000\r\n\
IsScoreRepeat=no\r\nIsScoreShuffle=no\r\nSoundLatency=9\r\nInGameMusic=yes\r\n\
[Network]\r\nNetID=ffff,ffff,ffff,\r\n\
[Player]\r\nName=Jos\xe9\r\n\
[Skirmish]\r\nCredits=10000\r\n";
        let profile = RetailOptionsProfile {
            game_speed: 3,
            scroll_rate: 2,
            tooltips: false,
            screen_width: 1280,
            screen_height: 720,
            sound_volume: 0.25,
            voice_volume: 0.75,
            score_volume: 0.5,
            ..RetailOptionsProfile::default()
        };
        let reads = Cell::new(0);
        let writes = Cell::new(0);
        let written = RefCell::new(None::<Vec<u8>>);
        let root = Path::new("C:/retail");

        profile
            .commit_settings_with(
                root,
                |path| {
                    reads.set(reads.get() + 1);
                    assert_eq!(path, root.join(profile_file::FILENAME));
                    Ok(input.to_vec())
                },
                |path, bytes| {
                    writes.set(writes.get() + 1);
                    assert_eq!(path, root.join(profile_file::FILENAME));
                    written.replace(Some(bytes.to_vec()));
                    Ok(())
                },
            )
            .unwrap();

        assert_eq!(reads.get(), 1);
        assert_eq!(writes.get(), 1);
        let output = written.borrow();
        let output = output.as_ref().unwrap();
        let parsed = IniFile::from_bytes(output).unwrap();
        let formatted = profile.formatted_sections();
        for (section_name, values) in [
            (OPTIONS_SECTION, formatted.options.as_slice()),
            (VIDEO_SECTION, formatted.video.as_slice()),
            (AUDIO_SECTION, formatted.audio.as_slice()),
            (NETWORK_SECTION, formatted.network.as_slice()),
            (VERA_SECTION, formatted.vera.as_slice()),
        ] {
            let section = parsed.section(section_name).unwrap();
            for (key, expected) in values {
                assert_eq!(
                    section.get_for_test(key),
                    (!expected.is_empty()).then_some(expected.as_str()),
                    "{section_name}/{key}"
                );
            }
        }
        assert!(
            output
                .windows(b"ToolTips=no\r\n".len())
                .any(|w| w == b"ToolTips=no\r\n")
        );
        assert!(
            output
                .windows(b"VoiceVolume=0.750000\r\n".len())
                .any(|w| w == b"VoiceVolume=0.750000\r\n")
        );
        assert!(
            output
                .windows(b"UnknownOption=keep\r\n".len())
                .any(|w| w == b"UnknownOption=keep\r\n")
        );
        // WriteToINI always rewrites NetID from the movie progress (-1, -1).
        assert!(
            output
                .windows(b"[Network]\r\nNetID=ffcf,ffdf,ffcf,\r\n".len())
                .any(|w| w == b"[Network]\r\nNetID=ffcf,ffdf,ffcf,\r\n")
        );
        assert!(
            output
                .windows(b"[Player]\r\nName=Jos\xe9\r\n".len())
                .any(|w| w == b"[Player]\r\nName=Jos\xe9\r\n")
        );
        assert!(
            output
                .windows(b"[Skirmish]\r\nCredits=10000\r\n".len())
                .any(|w| w == b"[Skirmish]\r\nCredits=10000\r\n")
        );
        assert!(
            !output
                .windows("AllowHiResModes".len())
                .any(|w| w == b"AllowHiResModes")
        );
        assert!(
            !output
                .windows("AllowModeToggle".len())
                .any(|w| w == b"AllowModeToggle")
        );
        assert!(
            !output
                .windows("AllowVRAMSidebar".len())
                .any(|w| w == b"AllowVRAMSidebar")
        );
    }

    #[test]
    fn unreadable_existing_profile_aborts_without_a_write() {
        let writes = Cell::new(0);
        let result = RetailOptionsProfile::default().commit_settings_with(
            Path::new("C:/retail"),
            |_| Err(io::Error::new(io::ErrorKind::PermissionDenied, "locked")),
            |_, _| {
                writes.set(writes.get() + 1);
                Ok(())
            },
        );
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(writes.get(), 0);
    }

    #[test]
    fn missing_profile_is_created_as_one_complete_snapshot() {
        let writes = Cell::new(0);
        let written = RefCell::new(Vec::new());
        RetailOptionsProfile::default()
            .commit_settings_with(
                Path::new("C:/retail"),
                |_| Err(io::Error::new(io::ErrorKind::NotFound, "missing")),
                |_, bytes| {
                    writes.set(writes.get() + 1);
                    written.replace(bytes.to_vec());
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(writes.get(), 1);
        let output = String::from_utf8(written.into_inner()).unwrap();
        assert!(output.starts_with("[Options]\r\nGameSpeed=3\r\n"));
        assert!(output.contains("[Video]\r\nScreenWidth=-1\r\n"));
        assert!(output.contains("[Audio]\r\nSoundVolume=0.700000\r\n"));
    }

    #[test]
    fn net_id_decodes_movie_progress_like_read_from_ini() {
        use crate::ui::movies_credits_shell::MovieProgress;
        let progress = |soviet, allied| MovieProgress { soviet, allied };
        let decode_net_id = |raw: Option<&str>| {
            let mut network = IniSection::new(NETWORK_SECTION.to_string());
            if let Some(raw) = raw {
                network.set("NetID", raw);
            }
            decode_net_id(&network)
        };
        // Retail default "0 0" and the obfuscated all-0xFFFF form.
        assert_eq!(decode_net_id(Some("ffcf,ffdf,ffcf,")), progress(-1, -1));
        assert_eq!(decode_net_id(Some("ffff,ffff,ffff,")), progress(-1, -1));
        assert_eq!(decode_net_id(None), progress(-1, -1));
        // "3 1": Soviet rows 0..=2, Allied row 0.
        assert_eq!(decode_net_id(Some("ffcc,ffdf,ffce,")), progress(2, 0));
        // Extra spaces are skipped by wcstok; one token leaves Allied at -1.
        assert_eq!(decode_net_id(Some("ffdf,ffc8,ffdf,ffdf,")), progress(6, -1));
        // "9 8": Soviet 8 is outside -1..=7 and falls back to -1; Allied 7 stays.
        assert_eq!(decode_net_id(Some("ffc6,ffdf,ffc7,")), progress(-1, 7));
    }

    #[test]
    fn net_id_encodes_like_write_to_ini_and_round_trips() {
        use crate::ui::movies_credits_shell::MovieProgress;
        assert_eq!(encode_net_id(MovieProgress::default()), "ffcf,ffdf,ffcf,");
        for soviet in -1..8 {
            for allied in -1..8 {
                let value = MovieProgress { soviet, allied };
                let mut network = IniSection::new(NETWORK_SECTION.to_string());
                network.set("NetID", &encode_net_id(value));
                assert_eq!(decode_net_id(&network), value);
            }
        }
    }
}
