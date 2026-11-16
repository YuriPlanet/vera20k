//! `uimd.ini [ToolTips]` (`src/Phobos.INI.cpp:145-170`) and the one
//! `[LoadingScreen]` key that shares the file (`:139-143`). The module owns
//! the values; the app reads them once at startup and hands them to the
//! tooltip text/geometry owner, mirroring Phobos' globals.

use super::ini_parser::IniFile;

/// `Phobos::UI::CostLabel` default `$` (`Phobos.INI.cpp:157`).
const COST_LABEL_DEFAULT: &str = "$";
/// `Phobos::UI::TimeLabel` default `⌚` (U+231A, `Phobos.INI.cpp:166`).
const TIME_LABEL_DEFAULT: &str = "\u{231A}";
/// `Phobos::UI::SWShotsFormat` default `Shots: %d` (`Phobos.INI.cpp:169`).
const SW_SHOTS_FORMAT_DEFAULT: &str = "Shots: %d";

/// The tooltip settings Phobos reads from `uimd.ini`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolTipsUi {
    /// `[ToolTips] ExtendedToolTips=`, default `no`. Replaces the single-line
    /// cameo tip with the multi-line name/cost/time/power text
    /// (`Misc/PhobosToolTip.cpp:124-154`).
    pub extended: bool,
    /// `[ToolTips] AnchoredToolTips=`, default `no`. Pins the super-weapon
    /// sidebar's tip beside its column instead of the cursor
    /// (`Misc/PhobosToolTip.cpp:367-406`).
    pub anchored: bool,
    /// `[ToolTips] MaxWidth=`, default `0` (the view bounds). `> 0` constrains
    /// the wrapped text and reserves five pixels of padding
    /// (`Misc/PhobosToolTip.cpp:305-353`).
    pub max_width: i32,
    /// `[ToolTips] CostLabel=`, resolved through CSF, default `$`.
    pub cost_label: String,
    /// `[ToolTips] TimeLabel=`, resolved through CSF, default `⌚`.
    pub time_label: String,
    /// `[ToolTips] SWShotsFormat=`, resolved through CSF, default
    /// `Shots: %d`. Kept here for the super-weapon line; no VERA consumer yet
    /// because `SW_Shots` has no owner (see the residual table).
    pub sw_shots_format: String,
    /// `[LoadingScreen] DisableEmptySpawnPositions=`, default `no`.
    pub disable_empty_spawn_positions: bool,
}

impl Default for ToolTipsUi {
    fn default() -> Self {
        Self {
            extended: false,
            anchored: false,
            max_width: 0,
            cost_label: COST_LABEL_DEFAULT.to_string(),
            time_label: TIME_LABEL_DEFAULT.to_string(),
            sw_shots_format: SW_SHOTS_FORMAT_DEFAULT.to_string(),
            disable_empty_spawn_positions: false,
        }
    }
}

impl ToolTipsUi {
    /// Phobos loads `uimd.ini` once at startup (`src/Phobos.INI.cpp:136-137`),
    /// so an unreadable file leaves every default in place.
    pub fn from_uimd_bytes(bytes: &[u8], lookup: &dyn Fn(&str) -> Option<String>) -> Self {
        let Ok(ini) = IniFile::from_bytes(bytes) else {
            log::warn!("Could not parse UIMD.INI for tooltips");
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
            extended: tooltips.is_some_and(|section| section.read_bool("ExtendedToolTips", false)),
            anchored: tooltips.is_some_and(|section| section.read_bool("AnchoredToolTips", false)),
            max_width: tooltips.map_or(0, |section| section.read_int("MaxWidth", 0)),
            cost_label: label("CostLabel", COST_LABEL_DEFAULT),
            time_label: label("TimeLabel", TIME_LABEL_DEFAULT),
            sw_shots_format: label("SWShotsFormat", SW_SHOTS_FORMAT_DEFAULT),
            disable_empty_spawn_positions: ini
                .section("LoadingScreen")
                .is_some_and(|section| section.read_bool("DisableEmptySpawnPositions", false)),
        }
    }
}

/// `GeneralUtils::LoadStringOrDefault` (`src/Utilities/GeneralUtils.cpp:37-43`):
/// a null, empty or `INIClass::IsBlank` value keeps the default; anything else
/// is looked up in the CSF table, whose miss stays visible as
/// `MISSING:'<key>'`.
fn load_string_or_default(
    section: &super::ini_parser::IniSection,
    key: &str,
    default: &str,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> String {
    let raw = section.read_string(key, "<none>", 2048);
    if raw.is_empty() || super::ini_parser::is_native_none_type_name(&raw) {
        return default.to_string();
    }
    lookup(&raw).unwrap_or_else(|| format!("MISSING:'{raw}'"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    #[test]
    fn defaults_match_the_native_constructor_and_readers() {
        let ui = ToolTipsUi::default();
        assert!(!ui.extended);
        assert!(!ui.anchored);
        assert_eq!(ui.max_width, 0);
        assert_eq!(ui.cost_label, "$");
        assert_eq!(ui.time_label, "\u{231A}");
        assert_eq!(ui.sw_shots_format, "Shots: %d");
        assert!(!ui.disable_empty_spawn_positions);
    }

    #[test]
    fn reads_the_tooltips_and_loading_screen_sections() {
        let ini = IniFile::from_str(
            "[ToolTips]\nExtendedToolTips=yes\nAnchoredToolTips=yes\nMaxWidth=420\n\
             CostLabel=TXT_COST\nTimeLabel=TXT_TIME\nSWShotsFormat=TXT_SHOTS\n\
             [LoadingScreen]\nDisableEmptySpawnPositions=yes\n",
        );
        let lookup = |key: &str| {
            Some(match key {
                "TXT_COST" => "CR",
                "TXT_TIME" => "T",
                "TXT_SHOTS" => "Left %d of %d",
                _ => return None,
            })
            .map(str::to_string)
        };
        let ui = ToolTipsUi::from_ini(&ini, &lookup);
        assert!(ui.extended);
        assert!(ui.anchored);
        assert_eq!(ui.max_width, 420);
        assert_eq!(ui.cost_label, "CR");
        assert_eq!(ui.time_label, "T");
        assert_eq!(ui.sw_shots_format, "Left %d of %d");
        assert!(ui.disable_empty_spawn_positions);
    }

    #[test]
    fn blank_labels_keep_the_default_but_a_csf_miss_stays_visible() {
        let ini = IniFile::from_str("[ToolTips]\nCostLabel=<none>\nTimeLabel=NO_SUCH_KEY\n");
        let ui = ToolTipsUi::from_ini(&ini, &|_| None);
        assert_eq!(ui.cost_label, "$");
        assert_eq!(ui.time_label, "MISSING:'NO_SUCH_KEY'");
        assert!(!ui.extended);
        assert!(!ui.disable_empty_spawn_positions);
    }
}
