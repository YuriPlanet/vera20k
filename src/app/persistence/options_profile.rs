//! Process-owned retail Options/Video/Audio profile transaction.
//!
//! This module translates the active-YR `OptionsClass` profile boundary into
//! one ordinary Rust value. It owns exact defaults, typed `RA2MD.INI` loading,
//! separate game-pair resolution and frontend selection, native-shaped formatting, and the one-read /
//! one-write preservation-safe commit. Runtime consumers remain in their
//! existing app, presentation, and audio owners.

use std::io;
use std::path::Path;

use crate::app::frontend::startup_options::{RetailStartupOptions, ScreenSize};
use crate::rules::ini_parser::IniFile;
use crate::util::ini_writer::set_ini_values;

pub(crate) const RA2MD_INI_FILENAME: &str = "RA2MD.INI";

const OPTIONS_SECTION: &str = "Options";
const VIDEO_SECTION: &str = "Video";
const AUDIO_SECTION: &str = "Audio";
const NETWORK_SECTION: &str = "Network";
/// `CRCEngine::operator()` @ `0x004A1DE0` over `"Network"`, executed natively
/// under Unicorn: the scratch value `INIClass__ReadCommaHexUTF16` leaves for a
/// failed first `%x` conversion on the fresh-file cache miss.
const NETWORK_SECTION_CRC: u32 = 0x70CA_A741;
/// Destination units `OptionsClass__ReadFromINI` passes for NetID (`push 0xC8`).
const NET_ID_UNITS: usize = 0xC8;
const SCREEN_SIZE_UNSET: i32 = -1;
const DEFAULT_SCREEN_WIDTH: i32 = 800;
const DEFAULT_SCREEN_HEIGHT: i32 = 600;
pub(crate) const RETAIL_SHELL_SIZE: ScreenSize = ScreenSize {
    width: 800,
    height: 600,
};

/// The process-lifetime subset of active-YR `OptionsClass` owned by
/// `RA2MD.INI` `[Options]`, `[Video]`, and `[Audio]`.
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
    /// Read and retained, but deliberately omitted by the native writer.
    pub(crate) allow_hi_res_modes: bool,
    /// Native stores this outside the contiguous Options object. It remains a
    /// read-only retained profile value here and is never serialized.
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
}

/// The two process-start products obtained from one physical `RA2MD.INI`
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
    /// `[Network] NetID` only. Native also writes Socket/NetCard/DestNet from
    /// network fields this profile does not model; their bytes are preserved.
    pub(crate) network: [(&'static str, String); 1],
}

impl RetailOptionsLoad {
    /// Replay startup with no profile snapshot.
    ///
    /// This is used when configuration is unavailable and by sealed capture
    /// after passing exact constructor defaults instead of operator argv.
    pub(crate) fn without_ra2md(startup: &RetailStartupOptions) -> Self {
        Self::from_ini_snapshot(startup, None)
    }

    /// Read and parse one physical `RA2MD.INI` snapshot, then replay native's
    /// early Video-only read and later complete profile read over it.
    ///
    /// Missing/unreadable/malformed input is non-fatal and follows the same
    /// no-snapshot path. Capture callers intentionally use
    /// [`Self::without_ra2md`] so user profile state cannot contaminate sealed
    /// capture output.
    pub(crate) fn from_ra2md(ra2_dir: &Path, startup: &RetailStartupOptions) -> Self {
        Self::from_ra2md_with(ra2_dir, startup, |path| std::fs::read(path))
    }

    /// Injectable form of [`Self::from_ra2md`] used to prove that both
    /// semantic reads share one physical filesystem read.
    fn from_ra2md_with<Read>(ra2_dir: &Path, startup: &RetailStartupOptions, read: Read) -> Self
    where
        Read: FnOnce(&Path) -> io::Result<Vec<u8>>,
    {
        let path = ra2_dir.join(RA2MD_INI_FILENAME);
        let bytes = match read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Self::without_ra2md(startup);
            }
            Err(error) => {
                log::warn!(
                    "Could not read {} for retail options: {error}",
                    path.display()
                );
                return Self::without_ra2md(startup);
            }
        };
        let ini = match IniFile::from_bytes(&bytes) {
            Ok(ini) => ini,
            Err(error) => {
                log::warn!(
                    "Could not parse {} for retail options: {error}",
                    path.display()
                );
                return Self::without_ra2md(startup);
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
    /// Seed native constructor defaults with the screen fields already chosen
    /// by the command-line switch pass.
    ///
    /// A later INI read uses these values as its per-key defaults, so a present
    /// profile key overrides the switch while a missing key leaves it intact.
    pub(crate) fn from_startup(startup: &RetailStartupOptions) -> Self {
        let mut profile = Self::default();
        profile.screen_width = startup.screen_width;
        profile.screen_height = startup.screen_height;
        profile
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
            self.score_volume =
                upper_only_volume(audio.read_double("ScoreVolume", f64::from(self.score_volume)));
            self.is_score_repeat = audio.read_bool("IsScoreRepeat", self.is_score_repeat);
            self.is_score_shuffle = audio.read_bool("IsScoreShuffle", self.is_score_shuffle);
            self.in_game_music = audio.read_bool("InGameMusic", self.in_game_music);
            // Native performs an ordinary signed ReadInt and stores the low
            // sixteen bits into the profile object after its intervening
            // Network NetID decode. Network semantics stay outside this slice.
            self.sound_latency =
                audio.read_int("SoundLatency", i32::from(self.sound_latency)) as u16;
        }

        self.movie_progress = decode_net_id(
            ini.section(NETWORK_SECTION)
                .and_then(|network| network.get("NetID")),
        );
    }

    /// Mutate the early-read live pair through startup fallback and return its
    /// window-safe pixel projection.
    ///
    /// If either field is still the exact `-1` sentinel, retail replaces both
    /// as one pair. The later full read may then reapply a present single key
    /// over this fallback for retained profile state. Explicit non-sentinel
    /// zero/negative values are retained, while their platform projection is
    /// bounded to one pixel.
    fn resolve_screen_size(&mut self) -> ScreenSize {
        // Retail provenance: pre-window screen-pair fallback — `WinMain` @
        // `0x006BB9A0`, branch `0x006BD94A..0x006BD9B5`.
        if self.screen_width == SCREEN_SIZE_UNSET || self.screen_height == SCREEN_SIZE_UNSET {
            self.screen_width = DEFAULT_SCREEN_WIDTH;
            self.screen_height = DEFAULT_SCREEN_HEIGHT;
        }
        self.game_screen_size()
    }

    /// Current match preference, including launcher edits, projected only at
    /// the platform boundary. Scenario start 00683DBB reads A8EB84/88 here.
    pub(crate) fn game_screen_size(&self) -> ScreenSize {
        ScreenSize {
            width: self.screen_width.max(1) as u32,
            height: self.screen_height.max(1) as u32,
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
            network: [("NetID", encode_net_id(self.movie_progress))],
        }
    }

    /// Transform one raw profile snapshot without performing filesystem I/O.
    pub(crate) fn transform_ra2md(&self, input: &[u8]) -> Vec<u8> {
        let formatted = self.formatted_sections();
        let output = apply_formatted_section(input, OPTIONS_SECTION, &formatted.options);
        let output = apply_formatted_section(&output, VIDEO_SECTION, &formatted.video);
        let output = apply_formatted_section(&output, AUDIO_SECTION, &formatted.audio);
        apply_formatted_section(&output, NETWORK_SECTION, &formatted.network)
    }

    /// Commit the complete modeled profile with one read and one write.
    ///
    /// `NotFound` is the only read failure that permits creating a new file.
    /// Refusing to replace other unreadable inputs prevents a partial snapshot
    /// from destroying profile bytes owned by other systems.
    pub(crate) fn commit_ra2md(&self, ra2_dir: &Path) -> io::Result<()> {
        self.commit_ra2md_with(
            ra2_dir,
            |path| std::fs::read(path),
            |path, bytes| std::fs::write(path, bytes),
        )
    }

    /// Injectable form of [`Self::commit_ra2md`] used to verify the physical
    /// one-read/one-write transaction without touching a user's profile.
    pub(crate) fn commit_ra2md_with<Read, Write>(
        &self,
        ra2_dir: &Path,
        read: Read,
        write: Write,
    ) -> io::Result<()>
    where
        Read: FnOnce(&Path) -> io::Result<Vec<u8>>,
        Write: FnOnce(&Path, &[u8]) -> io::Result<()>,
    {
        let path = ra2_dir.join(RA2MD_INI_FILENAME);
        let input = match read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error),
        };
        let output = self.transform_ra2md(&input);
        write(&path, &output)
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
fn decode_net_id(raw: Option<&str>) -> crate::ui::movies_credits_shell::MovieProgress {
    let units =
        crate::rules::ini_value::read_comma_hex_utf16(raw, &[], NET_ID_UNITS, NETWORK_SECTION_CRC);
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
    fn network_section_crc_is_the_crc_engine_of_its_name() {
        assert_eq!(
            crate::assets::mix_hash::crc_engine(NETWORK_SECTION.as_bytes()),
            NETWORK_SECTION_CRC
        );
    }

    fn load_snapshot(startup: &RetailStartupOptions, bytes: &[u8]) -> RetailOptionsLoad {
        RetailOptionsLoad::from_ra2md_with(Path::new("C:/retail"), startup, |_| Ok(bytes.to_vec()))
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
    fn width_only_snapshot_uses_fallback_window_then_rereads_retained_width() {
        let reads = Cell::new(0);
        let load = RetailOptionsLoad::from_ra2md_with(
            Path::new("C:/retail"),
            &RetailStartupOptions::default(),
            |path| {
                reads.set(reads.get() + 1);
                assert_eq!(path, Path::new("C:/retail").join(RA2MD_INI_FILENAME));
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
    fn height_only_snapshot_uses_fallback_window_then_rereads_retained_height() {
        let load = load_snapshot(
            &RetailStartupOptions::default(),
            b"[Video]\nScreenHeight=480\n",
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
            (800, 480)
        );
    }

    #[test]
    fn ordinary_frontend_size_is_independent_of_game_profile_and_launcher_edits() {
        for (width, height) in [(640, 480), (800, 600), (1024, 768)] {
            let ini = format!("[Video]\nScreenWidth={width}\nScreenHeight={height}\n");
            let mut load = load_snapshot(&RetailStartupOptions::default(), ini.as_bytes());
            assert_eq!(load.startup_shell_screen, RETAIL_SHELL_SIZE);
            assert_eq!(
                load.retained_profile.game_screen_size(),
                ScreenSize { width, height }
            );
            load.retained_profile.screen_width = 1280;
            load.retained_profile.screen_height = 960;
            assert_eq!(
                load.retained_profile.game_screen_size(),
                ScreenSize {
                    width: 1280,
                    height: 960
                }
            );
            assert_eq!(load.startup_shell_screen, RETAIL_SHELL_SIZE);
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
            load.retained_profile.game_screen_size(),
            ScreenSize {
                width: 1024,
                height: 768
            }
        );
    }

    #[test]
    fn configured_game_pair_does_not_select_the_shell_size() {
        let load = load_snapshot(
            &RetailStartupOptions::default(),
            b"[Video]\nScreenWidth=640\nScreenHeight=480\n",
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
            (640, 480)
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
        let missing = RetailOptionsLoad::from_ra2md_with(Path::new("C:/retail"), &complete, |_| {
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
        let partial = RetailOptionsLoad::without_ra2md(&partial);
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
    fn explicit_invalid_screen_values_are_retained_but_projected_safely() {
        let mut profile = RetailOptionsProfile {
            screen_width: 0,
            screen_height: -20,
            ..RetailOptionsProfile::default()
        };
        assert_eq!(
            profile.resolve_screen_size(),
            ScreenSize {
                width: 1,
                height: 1,
            }
        );
        assert_eq!((profile.screen_width, profile.screen_height), (0, -20));
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
            .commit_ra2md_with(
                root,
                |path| {
                    reads.set(reads.get() + 1);
                    assert_eq!(path, root.join(RA2MD_INI_FILENAME));
                    Ok(input.to_vec())
                },
                |path, bytes| {
                    writes.set(writes.get() + 1);
                    assert_eq!(path, root.join(RA2MD_INI_FILENAME));
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
        ] {
            let section = parsed.section(section_name).unwrap();
            for (key, expected) in values {
                assert_eq!(
                    section.get(key),
                    Some(expected.as_str()),
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
        let result = RetailOptionsProfile::default().commit_ra2md_with(
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
            .commit_ra2md_with(
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
                assert_eq!(decode_net_id(Some(&encode_net_id(value))), value);
            }
        }
    }
}
