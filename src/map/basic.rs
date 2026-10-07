//! Parser for the `[Basic]` map section.
//!
//! `[Basic]` carries scenario/map metadata used by the original games and tools.
//! This module keeps the first-pass support intentionally narrow: parse the most
//! useful metadata now and leave the broader scenario semantics for later work.

use crate::rules::ini_parser::IniFile;

/// Owner of the active `SpecialFlags::DestroyableBridges` bit during load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeDestroyabilityMode {
    /// Campaign and map-editor loads let map `[SpecialFlags]` override reset defaults.
    CampaignOrEditor,
    /// Skirmish/multiplayer replaces the active flag with session staging state.
    SkirmishOrMultiplayer { bridge_destruction: bool },
}

/// Parsed metadata from a map's `[Basic]` section.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BasicSection {
    /// Human-facing map/scenario name when present.
    pub name: Option<String>,
    /// Author text when present.
    pub author: Option<String>,
    /// String-table key or raw intro/briefing hook.
    pub intro: Option<String>,
    /// String-table key or raw briefing hook.
    pub briefing: Option<String>,
    /// Theme/music id requested by the map.
    pub theme: Option<String>,
    /// Declared INI format version used by the map.
    pub new_ini_format: Option<i32>,
    /// Whether tiberium/ore growth is enabled for this map (TiberiumGrowthEnabled=).
    pub tiberium_growth_enabled: Option<bool>,
    /// Native Scenario+34A4 (`0068A5E3..0068A61A`). Missing/invalid keys
    /// preserve the caller's prior value; a fresh scenario resets it to false.
    pub free_radar: Option<bool>,
    /// Native Scenario+34B4 (`ScenarioClass::Read_INI_Basic
    /// 0x0068A271..0x0068A289`, `Set_Defaults` false at `0x00683899`): the
    /// AI triggers of `AIMD.INI` never qualify (`sim::ai_team_creation`).
    pub ignore_global_ai_triggers: Option<bool>,
}

/// Parsed flags from a map's `[SpecialFlags]` section.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpecialFlagsSection {
    /// Parsed `MCVDeploy=` bit (0x0100); startup deployment logic is mode-specific elsewhere.
    pub mcv_deploy: Option<bool>,
    /// Parsed `InitialVeteran=` bit. The live starting-force path promotes
    /// successfully placed non-MCV units to elite when this bit is set.
    pub initial_veteran: Option<bool>,
    /// Map-level override: does ore grow denser? (TiberiumGrows=)
    pub tiberium_grows: Option<bool>,
    /// Map-level override: does ore spread to adjacent cells? (TiberiumSpreads=)
    pub tiberium_spreads: Option<bool>,
    /// Map-level override: are bridges destroyable? (DestroyableBridges=)
    pub destroyable_bridges: Option<bool>,
    /// Campaign/editor `ScenarioFlags` bit 0x20. While set, direct and area
    /// damage return before mutating world state (`Inert=`).
    pub inert: Option<bool>,
}

impl SpecialFlagsSection {
    /// Resolve the active `DestroyableBridges` bit using gamemd's mode ownership.
    pub fn effective_destroyable_bridges(&self, mode: BridgeDestroyabilityMode) -> bool {
        match mode {
            BridgeDestroyabilityMode::CampaignOrEditor => self.destroyable_bridges.unwrap_or(true),
            BridgeDestroyabilityMode::SkirmishOrMultiplayer { bridge_destruction } => {
                bridge_destruction
            }
        }
    }
}

/// Parse the `[Basic]` section from a map INI.
pub fn parse_basic_section(ini: &IniFile) -> BasicSection {
    let Some(section) = ini.section("Basic") else {
        return BasicSection::default();
    };

    BasicSection {
        // The scenario list's ReadString 0x40 (`0x00699858`).
        name: section.read_name("Name", 0x40).map(str::to_string),
        // No gamemd reader; VERA's scenario menu shows it.
        author: section.read_name("Author", 0x80).map(str::to_string),
        // ReadString 0x80 ahead of the movie and theme lookups (`0x0068A00C`,
        // `0x0068A02A`, `0x0068A138`).
        intro: section.read_name("Intro", 0x80).map(str::to_string),
        briefing: section.read_name("Brief", 0x80).map(str::to_string),
        theme: section.read_name("Theme", 0x80).map(str::to_string),
        // ReadInt(0) at `0x0068A151`.
        new_ini_format: section
            .is_present("NewINIFormat")
            .then(|| section.read_int("NewINIFormat", 0)),
        tiberium_growth_enabled: section.read_bool_value("TiberiumGrowthEnabled"),
        free_radar: section.read_bool_value("FreeRadar"),
        // ReadBool with the current Scenario+0x34B4 as default (`0x0068A284`).
        ignore_global_ai_triggers: section.read_bool_value("IgnoreGlobalAITriggers"),
    }
}

/// Parse the `[SpecialFlags]` section from a map INI.
pub fn parse_special_flags_section(ini: &IniFile) -> SpecialFlagsSection {
    let Some(section) = ini.section("SpecialFlags") else {
        return SpecialFlagsSection::default();
    };

    // ReadBool over each current flag (`0x006B8CEC`-`0x006B8E9D`).
    SpecialFlagsSection {
        mcv_deploy: section.read_bool_value("MCVDeploy"),
        initial_veteran: section.read_bool_value("InitialVeteran"),
        tiberium_grows: section.read_bool_value("TiberiumGrows"),
        tiberium_spreads: section.read_bool_value("TiberiumSpreads"),
        destroyable_bridges: section.read_bool_value("DestroyableBridges"),
        inert: section.read_bool_value("Inert"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    #[test]
    fn parse_basic_metadata() {
        let ini = IniFile::from_str(
            "[Basic]\nName=Mission One\nAuthor=Westwood\nIntro=TXT_M01INTRO\n\
             Brief=TXT_M01BRIEF\nTheme=BIGF226M\nNewINIFormat=4\n",
        );

        let basic = parse_basic_section(&ini);
        assert_eq!(basic.name.as_deref(), Some("Mission One"));
        assert_eq!(basic.author.as_deref(), Some("Westwood"));
        assert_eq!(basic.intro.as_deref(), Some("TXT_M01INTRO"));
        assert_eq!(basic.briefing.as_deref(), Some("TXT_M01BRIEF"));
        assert_eq!(basic.theme.as_deref(), Some("BIGF226M"));
        assert_eq!(basic.new_ini_format, Some(4));
    }

    #[test]
    fn missing_basic_section_returns_defaults() {
        let ini = IniFile::from_str("[Map]\nTheater=TEMPERATE\n");
        let basic = parse_basic_section(&ini);
        assert_eq!(basic, BasicSection::default());
    }

    #[test]
    fn free_radar_preserves_original_bool_tokens_and_supplied_default() {
        let original: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/free_radar_oracle/fixtures/native-free-radar.json",
        ))
        .unwrap();
        let cases = original["parser_cases"].as_array().unwrap();
        assert_eq!(cases.len(), 28);
        for case in cases {
            let key = case["token"]
                .as_str()
                .map(|token| format!("FreeRadar={token}\n"))
                .unwrap_or_default();
            let parsed =
                parse_basic_section(&IniFile::from_str(&format!("[Basic]\n{key}"))).free_radar;
            let prior = case["prior"].as_u64().unwrap() != 0;
            assert_eq!(
                parsed.unwrap_or(prior),
                case["stored"].as_u64().unwrap() != 0,
                "original CCINI token/default {case}"
            );
            if case["token"].is_null()
                || matches!(case["token"].as_str(), Some("" | "invalid" | "2"))
            {
                assert_eq!(parsed, None, "missing/invalid retains optional ownership");
            }
        }
        assert_eq!(
            parse_basic_section(&IniFile::from_str("[Map]\n")).free_radar,
            None
        );
    }

    #[test]
    fn parse_special_flags_bridge_override() {
        let ini = IniFile::from_str(
            "[SpecialFlags]\nMCVDeploy=yes\nInitialVeteran=yes\nTiberiumGrows=yes\nTiberiumSpreads=no\nDestroyableBridges=no\nInert=yes\n",
        );
        let flags = parse_special_flags_section(&ini);
        assert_eq!(flags.mcv_deploy, Some(true));
        assert_eq!(flags.initial_veteran, Some(true));
        assert_eq!(flags.tiberium_grows, Some(true));
        assert_eq!(flags.tiberium_spreads, Some(false));
        assert_eq!(flags.destroyable_bridges, Some(false));
        assert_eq!(flags.inert, Some(true));
    }

    #[test]
    fn gsi_04_10_special_flags_inert_is_optional_scenario_authority() {
        assert_eq!(
            parse_special_flags_section(&IniFile::from_str("[SpecialFlags]\nInert=yes\n")).inert,
            Some(true)
        );
        assert_eq!(
            parse_special_flags_section(&IniFile::from_str("[SpecialFlags]\nInert=no\n")).inert,
            Some(false)
        );
        assert_eq!(
            parse_special_flags_section(&IniFile::from_str("[Basic]\nName=Ordinary\n")).inert,
            None
        );
    }

    #[test]
    fn parse_special_flags_mcvdeploy_no() {
        let ini = IniFile::from_str("[SpecialFlags]\nMCVDeploy=no\n");
        let flags = parse_special_flags_section(&ini);
        assert_eq!(flags.mcv_deploy, Some(false));
        assert_eq!(flags.tiberium_grows, None);
        assert_eq!(flags.tiberium_spreads, None);
        assert_eq!(flags.destroyable_bridges, None);
    }

    #[test]
    fn missing_special_flags_mcvdeploy_defaults_to_none() {
        let ini = IniFile::from_str("[SpecialFlags]\nDestroyableBridges=yes\n");
        let flags = parse_special_flags_section(&ini);
        assert_eq!(flags.mcv_deploy, None);
        assert_eq!(flags.destroyable_bridges, Some(true));

        let ini = IniFile::from_str("[Basic]\nName=No Special Flags\n");
        assert_eq!(parse_special_flags_section(&ini).mcv_deploy, None);
    }

    #[test]
    fn specialflags_destroyablebridges_map_override_campaign_only() {
        let flags = SpecialFlagsSection {
            destroyable_bridges: Some(false),
            ..SpecialFlagsSection::default()
        };

        assert!(!flags.effective_destroyable_bridges(BridgeDestroyabilityMode::CampaignOrEditor));
        assert!(flags.effective_destroyable_bridges(
            BridgeDestroyabilityMode::SkirmishOrMultiplayer {
                bridge_destruction: true,
            }
        ));
    }

    #[test]
    fn skirmish_bridge_destruction_option_controls_specialflags_bit_8000() {
        let flags = SpecialFlagsSection {
            destroyable_bridges: Some(true),
            ..SpecialFlagsSection::default()
        };

        assert!(!flags.effective_destroyable_bridges(
            BridgeDestroyabilityMode::SkirmishOrMultiplayer {
                bridge_destruction: false,
            }
        ));
        assert!(
            SpecialFlagsSection::default().effective_destroyable_bridges(
                BridgeDestroyabilityMode::SkirmishOrMultiplayer {
                    bridge_destruction: true,
                }
            )
        );
    }
}
