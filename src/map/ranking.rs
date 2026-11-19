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

    /// Phobos `ScenarioExt::LoadFromINIFile` (`Ext/Scenario/Body.cpp:152-166`):
    /// a campaign scenario's own `MISSIONMD.INI` section (named by
    /// Scenario+0x125C, the loaded map file name) overrides each ranking field
    /// through `Ranking.<Name>`. Missing keys and values keep the map's
    /// `[Ranking]` result, which is the current field.
    #[cfg(feature = "phobos")]
    pub fn apply_missionmd_overrides(
        &mut self,
        campaign: bool,
        missionmd: &IniFile,
        scenario_section: &str,
    ) {
        if !campaign {
            return;
        }
        let Some(section) = missionmd.section(scenario_section) else {
            return;
        };
        self.par_time_easy_ticks =
            section.read_campaign_score_time("Ranking.ParTimeEasy", self.par_time_easy_ticks);
        self.par_time_medium_ticks =
            section.read_campaign_score_time("Ranking.ParTimeMedium", self.par_time_medium_ticks);
        self.par_time_hard_ticks =
            section.read_campaign_score_time("Ranking.ParTimeHard", self.par_time_hard_ticks);
        self.under_par_title =
            section.read_string("Ranking.UnderParTitle", &self.under_par_title, 0x1F);
        self.under_par_message =
            section.read_string("Ranking.UnderParMessage", &self.under_par_message, 0x1F);
        self.over_par_title =
            section.read_string("Ranking.OverParTitle", &self.over_par_title, 0x1F);
        self.over_par_message =
            section.read_string("Ranking.OverParMessage", &self.over_par_message, 0x1F);
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

    /// Phobos `ScenarioExt::LoadFromINIFile`: the scenario's own MISSIONMD
    /// section overrides the map's `[Ranking]` fields key by key.
    #[cfg(feature = "phobos")]
    #[test]
    fn missionmd_scenario_section_overrides_the_map_ranking() {
        let mut ranking = parse_ranking_section(&IniFile::from_str(
            "[Ranking]
ParTimeEasy=00:01:00
ParTimeHard=00:04:00
UnderParTitle=Map fast
",
        ));
        let missionmd = IniFile::from_str(
            "[C1A01MD.MAP]
Ranking.ParTimeEasy=00:02:00
Ranking.ParTimeMedium=00:03:00
             Ranking.UnderParTitle=Scenario fast
Ranking.UnderParMessage=Well done
             Ranking.OverParTitle=Scenario slow
Ranking.OverParMessage=Try again
             [OTHER]
Ranking.ParTimeEasy=09:00:00
",
        );
        ranking.apply_missionmd_overrides(true, &missionmd, "C1A01MD.MAP");
        assert_eq!(ranking.par_time_easy_ticks, 7_200);
        assert_eq!(ranking.par_time_medium_ticks, 10_800);
        assert_eq!(
            ranking.par_time_hard_ticks, 14_400,
            "an absent key keeps the map value"
        );
        assert_eq!(ranking.under_par_title, "Scenario fast");
        assert_eq!(ranking.under_par_message, "Well done");
        assert_eq!(ranking.over_par_title, "Scenario slow");
        assert_eq!(ranking.over_par_message, "Try again");
    }

    /// A scenario lacking a MISSIONMD section (or keys) keeps the map result.
    #[cfg(feature = "phobos")]
    #[test]
    fn missionmd_without_the_scenario_section_keeps_the_map_ranking() {
        let mut ranking = parse_ranking_section(&IniFile::from_str(
            "[Ranking]
ParTimeHard=00:02:00
OverParMessage=Map slow
",
        ));
        let before = ranking.clone();
        ranking.apply_missionmd_overrides(
            true,
            &IniFile::from_str(
                "[OTHER]
Ranking.ParTimeEasy=01:00:00
",
            ),
            "C1A01MD.MAP",
        );
        assert_eq!(ranking, before);
        ranking.apply_missionmd_overrides(
            true,
            &IniFile::from_str(
                "[C1A01MD.MAP]
Ranking.ParTimeEasy=bad
",
            ),
            "C1A01MD.MAP",
        );
        assert_eq!(
            ranking.par_time_easy_ticks, 0,
            "Read_Time parses an invalid present value as zero"
        );
        let mut skirmish = before.clone();
        skirmish.apply_missionmd_overrides(
            false,
            &IniFile::from_str(
                "[C1A01MD.MAP]
Ranking.ParTimeEasy=01:00:00
",
            ),
            "C1A01MD.MAP",
        );
        assert_eq!(skirmish, before, "a non-campaign launch ignores MISSIONMD");
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
