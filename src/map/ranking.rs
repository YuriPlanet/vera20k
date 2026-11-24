//! Parser for the map `[Ranking]` fields consumed by campaign dialog `0x103`.
//!
//! `ScenarioClass::Read_INI` reads these map-owned values at `0x0068A741..812`.
//! The live `ScenarioClass` fields remain the native authority; this parsed
//! projection is retained only so the app can render the corresponding result
//! page after the scenario has ended or been restored from a save.

use crate::rules::ini_parser::IniFile;

const DEFAULT_PAR_TIME_TICKS: i32 = 0xE10;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RankingSection {
    /// Scenario+0x35A4; `[Ranking] ParTimeEasy`, clock ticks (60 per second).
    pub par_time_easy_ticks: i32,
    /// Scenario+0x35A8; `[Ranking] ParTimeMedium`, clock ticks (60 per second).
    pub par_time_medium_ticks: i32,
    /// Scenario+0x35AC; `[Ranking] ParTimeHard`, clock ticks (60 per second).
    pub par_time_hard_ticks: i32,
    /// Scenario+0x35B0; native ReadString capacity 0x1F.
    pub under_par_title: String,
    /// Scenario+0x35CF; native ReadString capacity 0x1F.
    pub under_par_message: String,
    /// Scenario+0x35EE; native ReadString capacity 0x1F.
    pub over_par_title: String,
    /// Scenario+0x360D; native ReadString capacity 0x1F.
    pub over_par_message: String,
}

impl Default for RankingSection {
    fn default() -> Self {
        Self {
            par_time_easy_ticks: DEFAULT_PAR_TIME_TICKS,
            par_time_medium_ticks: DEFAULT_PAR_TIME_TICKS,
            par_time_hard_ticks: DEFAULT_PAR_TIME_TICKS,
            under_par_title: "GUI:NoUnderParTitle".to_string(),
            under_par_message: "GUI:NoUnderParMessage".to_string(),
            over_par_title: "GUI:NoOverParTitle".to_string(),
            over_par_message: "GUI:NoOverParMessage".to_string(),
        }
    }
}

impl RankingSection {
    pub fn par_time_for_difficulty(&self, difficulty: i32) -> i32 {
        match difficulty {
            1 => self.par_time_medium_ticks,
            2 => self.par_time_hard_ticks,
            _ => self.par_time_easy_ticks,
        }
    }
}

/// Read gamemd's `[Ranking]` fields without substituting retail values for
/// native constructor defaults. Par times use the custom `Read_Time` reader
/// `0x0052A760`; string defaults and 0x1F capacities come from
/// `ScenarioClass::Read_INI` at `0x0068A741..812`.
pub fn parse_ranking_section(ini: &IniFile) -> RankingSection {
    let Some(section) = ini.section("Ranking") else {
        return RankingSection::default();
    };
    RankingSection {
        par_time_easy_ticks: section
            .read_campaign_score_time("ParTimeEasy", DEFAULT_PAR_TIME_TICKS),
        par_time_medium_ticks: section
            .read_campaign_score_time("ParTimeMedium", DEFAULT_PAR_TIME_TICKS),
        par_time_hard_ticks: section
            .read_campaign_score_time("ParTimeHard", DEFAULT_PAR_TIME_TICKS),
        // ScenarioClass::Read_INI passes these exact GUI:-qualified literal
        // defaults to ReadString; 0x734E60 does not add a namespace later.
        under_par_title: section.read_string("UnderParTitle", "GUI:NoUnderParTitle", 0x1F),
        under_par_message: section.read_string("UnderParMessage", "GUI:NoUnderParMessage", 0x1F),
        over_par_title: section.read_string("OverParTitle", "GUI:NoOverParTitle", 0x1F),
        over_par_message: section.read_string("OverParMessage", "GUI:NoOverParMessage", 0x1F),
    }
}

#[cfg(all(test, tests_map))]
mod tests {
    use super::*;

    #[test]
    fn reads_native_campaign_ranking_defaults_and_values() {
        let default = parse_ranking_section(&IniFile::from_str("[Basic]\nName=Mission\n"));
        assert_eq!(default.par_time_easy_ticks, 0xE10);
        assert_eq!(default.par_time_medium_ticks, 0xE10);
        assert_eq!(default.par_time_hard_ticks, 0xE10);
        assert_eq!(default.under_par_title, "GUI:NoUnderParTitle");
        assert_eq!(default.over_par_message, "GUI:NoOverParMessage");

        let parsed = parse_ranking_section(&IniFile::from_str(
            "[Ranking]\nParTimeEasy=00:02:30\nParTimeMedium=01:00:00\nParTimeHard=02:30:15\nUnderParTitle=Fast rank\nUnderParMessage=Well done\nOverParTitle=Slow rank\nOverParMessage=Try again\n",
        ));
        assert_eq!(parsed.par_time_easy_ticks, 9_000);
        assert_eq!(parsed.par_time_medium_ticks, 216_000);
        assert_eq!(parsed.par_time_hard_ticks, 540_900);
        assert_eq!(parsed.under_par_title, "Fast rank");
        assert_eq!(parsed.under_par_message, "Well done");
        assert_eq!(parsed.over_par_title, "Slow rank");
        assert_eq!(parsed.over_par_message, "Try again");
        assert_eq!(parsed.par_time_for_difficulty(0), 9_000);
        assert_eq!(parsed.par_time_for_difficulty(1), 216_000);
        assert_eq!(parsed.par_time_for_difficulty(2), 540_900);
    }

    #[test]
    fn present_ranking_section_uses_native_gui_fallbacks_for_missing_strings() {
        let parsed = parse_ranking_section(&IniFile::from_str("[Ranking]\nParTimeHard=00:04:00\n"));

        assert_eq!(parsed.par_time_hard_ticks, 14_400);
        assert_eq!(parsed.under_par_title, "GUI:NoUnderParTitle");
        assert_eq!(parsed.under_par_message, "GUI:NoUnderParMessage");
        assert_eq!(parsed.over_par_title, "GUI:NoOverParTitle");
        assert_eq!(parsed.over_par_message, "GUI:NoOverParMessage");
    }
}
