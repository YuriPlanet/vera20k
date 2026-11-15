//! Phobos `uimd.ini [ToolTips]` and the loading-screen spawn-position switch.
//!
//! `src/Phobos.INI.cpp:135-170` parses `uimd.ini` once at startup:
//!
//! - `[LoadingScreen] DisableEmptySpawnPositions=` (default `no`) makes the
//!   loading screen skip the hardcoded black dots gamemd draws over start
//!   positions (`Misc/Hooks.UI.cpp:29-37`, hook `0x640B8D` on the
//!   `jle 0x00640CE2` that guards the 4x4 black fill loop).
//! - `[ToolTips] ExtendedToolTips=` (default `no`) switches every cameo tip to
//!   Phobos' own text (`Misc/PhobosToolTip.cpp:19-21` gates `HelpText_Techno`
//!   and `HelpText_Super`).
//! - `AnchoredToolTips=` (default `no`) moves the cameo tip next to the sidebar
//!   or superweapon-sidebar column (`Misc/PhobosToolTip.cpp:391-450`).
//! - `MaxWidth=` (default `0`) replaces the draw pass's wrap limit for cameo
//!   tips (`Misc/PhobosToolTip.cpp:333-345`, hook `0x478EF8`).
//! - `CostLabel=`, `TimeLabel=` and `SWShotsFormat=` name the labels the
//!   extended text prints. Each `ReadString` defaults to `<none>`, so only a
//!   blank or `<none>` value keeps the literal default; any other value is a
//!   CSF label resolved through `GeneralUtils::LoadStringOrDefault`
//!   (`src/Utilities/GeneralUtils.cpp:37-43`) at the same startup moment.
//!
//! The `PowerLabel`/`PowerBlackoutLabel` keys of the same native block are read
//! by the sidebar-indicator owner (`phobos_sidebar_indicators.rs`), which also
//! owns the `TextAlign` decoding they share.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on app/, render/, ui/, sim/ or audio/.

use super::ini_parser::IniFile;
use super::phobos_sidebar_indicators::load_string_or_default;

/// Phobos' literal `"$"` cost prefix (`Phobos.INI.cpp:158`).
const COST_LABEL_DEFAULT: &str = "$";
/// The `⌚` glyph (U+231A), `Phobos.INI.cpp:166`.
const TIME_LABEL_DEFAULT: &str = "\u{231A}";
/// `SWShotsFormat`, `Phobos.INI.cpp:169`.
const SW_SHOTS_FORMAT_DEFAULT: &str = "Shots: %d";

/// Everything the `[ToolTips]` block and `[LoadingScreen]` contribute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolTipsUi {
    /// `[ToolTips] ExtendedToolTips=`, default `no`: use Phobos' cameo text.
    pub extended_tooltips: bool,
    /// `[ToolTips] AnchoredToolTips=`, default `no`: anchor the cameo tip to
    /// the sidebar instead of the cursor.
    pub anchored_tooltips: bool,
    /// `[ToolTips] MaxWidth=`, default `0`: wrap limit in pixels; at or below
    /// zero keeps the draw pass's own limit.
    pub max_width: i32,
    /// `[ToolTips] CostLabel=`, resolved text, default `$`.
    pub cost_label: String,
    /// `[ToolTips] TimeLabel=`, resolved text, default `⌚` (U+231A).
    pub time_label: String,
    /// `[ToolTips] SWShotsFormat=`, resolved text, default `Shots: %d`.
    pub sw_shots_format: String,
    /// `[LoadingScreen] DisableEmptySpawnPositions=`, default `no`.
    pub disable_empty_spawn_positions: bool,
}

impl Default for ToolTipsUi {
    fn default() -> Self {
        Self {
            extended_tooltips: false,
            anchored_tooltips: false,
            max_width: 0,
            cost_label: COST_LABEL_DEFAULT.to_string(),
            time_label: TIME_LABEL_DEFAULT.to_string(),
            sw_shots_format: SW_SHOTS_FORMAT_DEFAULT.to_string(),
            disable_empty_spawn_positions: false,
        }
    }
}

impl ToolTipsUi {
    /// Phobos loads `uimd.ini` once at startup (`Phobos.INI.cpp:136-137`), so an
    /// unreadable file leaves every default in place. `lookup` resolves a label
    /// through the loaded CSF table, which Phobos' `StringTable::LoadString`
    /// asks at the same moment.
    pub fn from_uimd_bytes(bytes: &[u8], lookup: &dyn Fn(&str) -> Option<String>) -> Self {
        let Ok(ini) = IniFile::from_bytes(bytes) else {
            log::warn!("Could not parse UIMD.INI for tooltip settings");
            return Self::default();
        };
        Self::from_ini(&ini, lookup)
    }

    pub fn from_ini(ini: &IniFile, lookup: &dyn Fn(&str) -> Option<String>) -> Self {
        let defaults = Self::default();
        let tooltips = ini.section("ToolTips");
        Self {
            extended_tooltips: tooltips.is_some_and(|section| {
                section.read_bool("ExtendedToolTips", defaults.extended_tooltips)
            }),
            anchored_tooltips: tooltips.is_some_and(|section| {
                section.read_bool("AnchoredToolTips", defaults.anchored_tooltips)
            }),
            max_width: tooltips.map_or(defaults.max_width, |section| {
                section.read_int("MaxWidth", defaults.max_width)
            }),
            cost_label: tooltips.map_or_else(
                || defaults.cost_label.clone(),
                |section| load_string_or_default(section, "CostLabel", COST_LABEL_DEFAULT, lookup),
            ),
            time_label: tooltips.map_or_else(
                || defaults.time_label.clone(),
                |section| load_string_or_default(section, "TimeLabel", TIME_LABEL_DEFAULT, lookup),
            ),
            sw_shots_format: tooltips.map_or_else(
                || defaults.sw_shots_format.clone(),
                |section| {
                    load_string_or_default(
                        section,
                        "SWShotsFormat",
                        SW_SHOTS_FORMAT_DEFAULT,
                        lookup,
                    )
                },
            ),
            disable_empty_spawn_positions: ini.section("LoadingScreen").is_some_and(|section| {
                section.read_bool(
                    "DisableEmptySpawnPositions",
                    defaults.disable_empty_spawn_positions,
                )
            }),
        }
    }
}

#[cfg(test)]
#[path = "phobos_tooltips_ui_tests.rs"]
mod tests;
