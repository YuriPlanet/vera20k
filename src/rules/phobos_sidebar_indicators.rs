//! Phobos's sidebar text indicators, all three drawn by the one hook
//! `DEFINE_HOOK(0x4A25E0, CreditsClass_GraphicLogic_HarvesterCounter)`
//! (`src/Misc/Hooks.UI.cpp`). This module owns the keys; the presentation
//! owner turns them into the strings and colours the sidebar text lane draws.
//!
//! Two INI layers feed one draw site:
//! - `uimd.ini [Sidebar]` / `[ToolTips]` — the mod-wide switches, thresholds
//!   and label keys, read once at startup (`src/Phobos.INI.cpp:159-199`).
//! - `rulesmd.ini [<Side ID>] Sidebar.<Indicator>.*` — per-side offset, colours
//!   and alignment, read for every registered side by `SideExt::LoadFromINIFile`
//!   (`src/Ext/Side/Body.cpp:21-47`, driven from the `SideClass_LoadAllFromINI`
//!   hook at `0x679A10`).
//!
//! RESIDUAL: a label key the CSF table does not hold renders `MISSING:<key>`
//! in native (`StringTable::LoadString`); VERA keeps the glyph default instead.
//! `Sidebar.PowerDelta.Align=none` selects Phobos' `TextAlign::None` (`0xFFF`),
//! whose extra bits are font flags; VERA treats unparsable text as rejected and
//! keeps the previous value.

use super::ini_parser::{IniFile, IniSection};

/// The alignment bits `Sidebar.PowerDelta.Align=` selects, which Phobos ORs
/// straight into the `TextPrintType` flags (`src/Utilities/Enum.h:234-241`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SidebarTextAlign {
    /// `0x000` — the string starts at the anchor.
    #[default]
    Left,
    /// `0x100` — the string is centred on the anchor.
    Center,
    /// `0x200` — the string ends at the anchor.
    Right,
}

impl SidebarTextAlign {
    /// Phobos' `read<TextAlign>` (`src/Utilities/TemplateDef.h:1089-1099`):
    /// `_stricmp` against `left`, `center`, `centre` and `right`.
    pub fn parse(raw: &str) -> Option<Self> {
        Some(if raw.eq_ignore_ascii_case("left") {
            Self::Left
        } else if raw.eq_ignore_ascii_case("center") || raw.eq_ignore_ascii_case("centre") {
            Self::Center
        } else if raw.eq_ignore_ascii_case("right") {
            Self::Right
        } else {
            return None;
        })
    }
}

/// `uimd.ini [Sidebar] PowerDelta.*` (`src/Phobos.INI.cpp:192-199`). Phobos
/// keeps both thresholds as `double` and compares them against a `double`
/// ratio, so VERA holds `f64` here too; a `%` suffix is scaled by `ReadDouble`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PowerDeltaUi {
    /// `PowerDelta.Show=`, default `no`.
    pub show: bool,
    /// `PowerDelta.ConditionYellow=`, default `0.75`.
    pub condition_yellow: f64,
    /// `PowerDelta.ConditionRed=`, default `1.0`.
    pub condition_red: f64,
}

impl Default for PowerDeltaUi {
    fn default() -> Self {
        Self {
            show: false,
            condition_yellow: 0.75,
            condition_red: 1.0,
        }
    }
}

/// Everything `uimd.ini` contributes to the indicator hook, with the two
/// `[ToolTips]` labels already resolved to text.
#[derive(Debug, Clone, PartialEq)]
pub struct SidebarIndicatorsUi {
    pub power_delta: PowerDeltaUi,
    /// `[ToolTips] PowerLabel=`, default the `⚡` glyph (U+26A1).
    pub power_label: String,
    /// `[ToolTips] PowerBlackoutLabel=`, default `⚡❌` (U+26A1 U+274C).
    pub power_blackout_label: String,
}

impl Default for SidebarIndicatorsUi {
    /// Phobos always reaches the `[ToolTips]` read, so even an absent `uimd.ini`
    /// ends up with the two glyph labels rather than empty strings.
    fn default() -> Self {
        Self {
            power_delta: PowerDeltaUi::default(),
            power_label: POWER_LABEL_DEFAULT.to_string(),
            power_blackout_label: POWER_BLACKOUT_LABEL_DEFAULT.to_string(),
        }
    }
}

impl SidebarIndicatorsUi {
    /// Phobos loads `uimd.ini` once at startup
    /// (`src/Phobos.INI.cpp:136-137`), so an unreadable file leaves every
    /// default in place rather than failing the load. `lookup` resolves a
    /// `[ToolTips]` key through the loaded CSF table, which Phobos'
    /// `StringTable::LoadString` asks at the same moment.
    pub fn from_uimd_bytes(bytes: &[u8], lookup: &dyn Fn(&str) -> Option<String>) -> Self {
        let Ok(ini) = IniFile::from_bytes(bytes) else {
            log::warn!("Could not parse UIMD.INI for sidebar indicators");
            return Self::default();
        };
        Self::from_ini(&ini, lookup)
    }

    pub fn from_ini(ini: &IniFile, lookup: &dyn Fn(&str) -> Option<String>) -> Self {
        let tooltips = ini.section("ToolTips");
        let label = |key: &str, default: &str| {
            tooltips.map_or_else(
                || default.to_string(),
                |section| load_string_or_default(section, key, default, lookup),
            )
        };
        Self {
            power_delta: ini
                .section("Sidebar")
                .map(PowerDeltaUi::from_section)
                .unwrap_or_default(),
            power_label: label("PowerLabel", POWER_LABEL_DEFAULT),
            power_blackout_label: label("PowerBlackoutLabel", POWER_BLACKOUT_LABEL_DEFAULT),
        }
    }
}

/// `⚡`, the `defaultValue` of `Phobos.INI.cpp:160`.
const POWER_LABEL_DEFAULT: &str = "\u{26A1}";
/// `⚡❌`, the `defaultValue` of `Phobos.INI.cpp:163`.
const POWER_BLACKOUT_LABEL_DEFAULT: &str = "\u{26A1}\u{274C}";

/// `GeneralUtils::LoadStringOrDefault` (`src/Utilities/GeneralUtils.cpp:37-43`)
/// over `INI_EX::ReadString` with the `<none>` default: a blank value — absent,
/// empty or the `<none>` token — keeps the glyph default, anything else asks
/// the string table.
fn load_string_or_default(
    section: &IniSection,
    key: &str,
    default: &str,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> String {
    let raw = section.read_string(key, "<none>", 2048);
    if raw.is_empty() || raw.eq_ignore_ascii_case("<none>") {
        return default.to_string();
    }
    lookup(&raw).unwrap_or_else(|| default.to_string())
}

impl PowerDeltaUi {
    fn from_section(section: &IniSection) -> Self {
        let defaults = Self::default();
        Self {
            show: section.read_bool("PowerDelta.Show", defaults.show),
            condition_yellow: section
                .read_double("PowerDelta.ConditionYellow", defaults.condition_yellow),
            condition_red: section.read_double("PowerDelta.ConditionRed", defaults.condition_red),
        }
    }
}

/// `rulesmd.ini [<Side ID>] Sidebar.PowerDelta.*` (`src/Ext/Side/Body.cpp:42-47`,
/// types and defaults at `Body.h:38-43` and `:69-74`). Every field is a
/// `Valueable`, so each has a stored default and no fallback colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PowerDeltaSideSettings {
    /// `Sidebar.PowerDelta.Offset=`, `Point2D` (`Read2Integers`), default
    /// `0,0`, added to the hook's own anchor.
    pub offset: [i32; 2],
    /// `ColorGreen=`, default `0,255,0`.
    pub color_green: [u8; 3],
    /// `ColorYellow=`, default `255,255,0`. Also the colour when both output
    /// and drain are zero.
    pub color_yellow: [u8; 3],
    /// `ColorRed=`, default `255,0,0`.
    pub color_red: [u8; 3],
    /// `ColorGrey=`, default `128,128,128`: the spy / force-shield blackout.
    pub color_grey: [u8; 3],
    /// `Align=`, default `left`.
    pub align: SidebarTextAlign,
}

impl Default for PowerDeltaSideSettings {
    fn default() -> Self {
        Self {
            offset: [0, 0],
            color_green: [0, 255, 0],
            color_yellow: [255, 255, 0],
            color_red: [255, 0, 0],
            color_grey: [128, 128, 128],
            align: SidebarTextAlign::default(),
        }
    }
}

impl PowerDeltaSideSettings {
    /// A side with no section keeps the constructor defaults, as `SideExt`'s
    /// `if (!pINI->GetSection(pSection)) return;` does.
    pub(crate) fn from_section(section: &IniSection) -> Self {
        let defaults = Self::default();
        let mut this = Self {
            offset: section
                .read_int_fields_complete::<2>("Sidebar.PowerDelta.Offset", 2048)
                .unwrap_or(defaults.offset),
            color_green: section
                .read_phobos_color("Sidebar.PowerDelta.ColorGreen", defaults.color_green),
            color_yellow: section
                .read_phobos_color("Sidebar.PowerDelta.ColorYellow", defaults.color_yellow),
            color_red: section.read_phobos_color("Sidebar.PowerDelta.ColorRed", defaults.color_red),
            color_grey: section
                .read_phobos_color("Sidebar.PowerDelta.ColorGrey", defaults.color_grey),
            align: defaults.align,
        };
        let raw = section.read_string("Sidebar.PowerDelta.Align", "", 2048);
        if let Some(align) = SidebarTextAlign::parse(&raw) {
            this.align = align;
        }
        this
    }
}

#[cfg(test)]
#[path = "phobos_sidebar_indicators_tests.rs"]
mod tests;
