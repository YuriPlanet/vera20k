//! Scenario-owned campaign loading fields read from `MISSIONMD.INI`.
//!
//! `ScenarioClass::Full_Init @ 0x00687000..0x006873AB` reads the exact
//! `Scenario.FileName` section on each admitted MISSION pass. Strings are
//! cleared before their presence gates; coordinates use the current field as
//! their default. `SetDefaults @ 0x006839FE..0x00683A2E` initializes both to
//! empty/zero. Native execution is retained in
//! `tools/input_oracle/campaign_start.{json,meta.json}`.

use crate::rules::ini_parser::IniFile;

/// Full_Init686D80..687000's retained mission names. They are presentation
/// values, resolved against the active CSF at admission and saved verbatim;
/// restoring a game must not retranslate them through a different string table.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CampaignScenarioNames {
    name: Vec<u16>,
    ui_name: String,
    save_title: Vec<u16>,
}

impl CampaignScenarioNames {
    /// Call only after MISSIONMD opened. A missing file bypasses this entire
    /// native block; a present file with no section clears Name/UIName while
    /// retaining the previous save title.
    pub fn apply_ini(
        &mut self,
        ini: &IniFile,
        filename: &str,
        csf: &crate::assets::csf_file::CsfFile,
    ) {
        self.name.clear();
        self.ui_name.clear();
        let Some(section) = ini.section(filename) else {
            return;
        };
        self.name = section.read_comma_hex_utf16("Name", &[], 45);
        if section.is_present("UIName") {
            self.ui_name = section.read_string("UIName", "", 32);
        }
        if self.ui_name.is_empty() {
            return;
        }
        // 686F37..686F6C and686F73..686FF9 copy45 units then explicitly
        // terminate at index44. Retain UTF-16 so a split surrogate is not
        // silently changed before the presentation consumer sees it.
        self.name = csf
            .text(&self.ui_name)
            .encode_utf16()
            .take_while(|&unit| unit != 0)
            .take(44)
            .collect();
        self.save_title = csf
            .text(&format!("{}Sav", self.ui_name))
            .encode_utf16()
            .take_while(|&unit| unit != 0)
            .take(44)
            .collect();
        if self
            .save_title
            .starts_with(&[77, 73, 83, 83, 73, 78, 71, 58])
        {
            self.save_title.clone_from(&self.name);
        }
    }

    pub fn name_units(&self) -> &[u16] {
        &self.name
    }

    pub fn ui_name(&self) -> &str {
        &self.ui_name
    }

    pub fn save_title_units(&self) -> &[u16] {
        &self.save_title
    }

    pub fn save_title(&self) -> String {
        String::from_utf16_lossy(&self.save_title)
    }
}

/// The CSF key the mission restate briefing shows
/// (`ScenarioClass::ShowMissionRestateBriefing 0x0065F520`, Restate.cpp): the
/// `Briefing` entry of the scenario file name's section, read with
/// `ReadString(…, "", buf, 0x100)`, or `Brief:Error` when the section or key
/// is absent (`FindEntryByNames 0x00526B10`). A missing MISSIONMD opens
/// nothing; that is the caller's.
pub fn restate_briefing_key(ini: &IniFile, filename: &str) -> String {
    match ini
        .section(filename)
        .filter(|section| section.is_present("Briefing"))
    {
        Some(section) => section.read_string("Briefing", "", 0x100),
        None => "Brief:Error".to_owned(),
    }
}

/// One scenario's retained MISSION loading fields. Source selection and pass
/// ordering belong to the scenario loader; this owner only applies that pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CampaignLoadingMetadata {
    load_message_key: String,
    load_briefing_key: String,
    brief_location_640: [i32; 2],
    brief_location_800: [i32; 2],
    background_name_640: String,
    background_name_800: String,
    background_palette_name: String,
}

impl CampaignLoadingMetadata {
    #[cfg(any(test, feature = "test-support"))]
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply the original loading-field reads in their native order.
    ///
    /// The filename and keys are exact-case. Call this when Full_Init admits
    /// the MISSION input, including a pass whose filename section is absent;
    /// do not replace it with an overlay of scenario Rules inputs.
    pub fn apply_ini(&mut self, ini: &IniFile, filename: &str) {
        self.load_message_key.clear();
        self.load_briefing_key.clear();
        self.background_name_640.clear();
        self.background_name_800.clear();
        self.background_palette_name.clear();

        let Some(section) = ini.section(filename) else {
            return;
        };
        if section.is_present("LSLoadMessage") {
            self.load_message_key = section.read_string("LSLoadMessage", "", 31);
        }
        if section.is_present("LSLoadBriefing") {
            self.load_briefing_key = section.read_string("LSLoadBriefing", "", 31);
        }
        if section.is_present("LS640BriefLocX") {
            self.brief_location_640[0] =
                section.read_int("LS640BriefLocX", self.brief_location_640[0]);
        }
        if section.is_present("LS640BriefLocY") {
            self.brief_location_640[1] =
                section.read_int("LS640BriefLocY", self.brief_location_640[1]);
        }
        if section.is_present("LS800BriefLocX") {
            self.brief_location_800[0] =
                section.read_int("LS800BriefLocX", self.brief_location_800[0]);
        }
        if section.is_present("LS800BriefLocY") {
            self.brief_location_800[1] =
                section.read_int("LS800BriefLocY", self.brief_location_800[1]);
        }
        if section.is_present("LS640BkgdName") {
            self.background_name_640 = section.read_string("LS640BkgdName", "", 64);
        }
        if section.is_present("LS800BkgdName") {
            self.background_name_800 = section.read_string("LS800BkgdName", "", 64);
        }
        if section.is_present("LS800BkgdPal") {
            self.background_palette_name = section.read_string("LS800BkgdPal", "", 64);
        }
    }

    pub fn load_message_key(&self) -> &str {
        &self.load_message_key
    }

    pub fn load_briefing_key(&self) -> &str {
        &self.load_briefing_key
    }

    pub const fn brief_location_640(&self) -> [i32; 2] {
        self.brief_location_640
    }

    pub const fn brief_location_800(&self) -> [i32; 2] {
        self.brief_location_800
    }

    pub fn background_name_640(&self) -> &str {
        &self.background_name_640
    }

    pub fn background_name_800(&self) -> &str {
        &self.background_name_800
    }

    /// The native reader provides only `LS800BkgdPal`; both art widths use it.
    pub fn background_palette_name(&self) -> &str {
        &self.background_palette_name
    }
}

#[cfg(test)]
mod tests {
    use super::{CampaignLoadingMetadata, CampaignScenarioNames, restate_briefing_key};
    use crate::rules::ini_parser::IniFile;
    use serde_json::Value;

    #[test]
    fn campaign_mission_names_match_original_reader_and_retention() {
        let native = crate::test_fixture::json("tools/input_oracle/campaign_mission_titles.json");
        let entries: Vec<_> = native["csf_entries"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str().unwrap()))
            .collect();
        let csf = crate::assets::csf_file::test_csf(&entries);
        let units = |value: &Value| {
            value
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_u64().unwrap() as u16)
                .collect::<Vec<_>>()
        };
        for row in native["name_rows"].as_array().unwrap() {
            let mut names = CampaignScenarioNames {
                name: units(&row["prior_name_utf16"]),
                ui_name: row["prior_ui_name"].as_str().unwrap().into(),
                save_title: units(&row["prior_save_title_utf16"]),
            };
            if row["mission_file_exists"].as_bool().unwrap() {
                let mut text = String::new();
                for (name, section) in row["sections"].as_object().unwrap() {
                    text.push_str(&format!("[{name}]\n"));
                    for (key, value) in section.as_object().unwrap() {
                        text.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
                    }
                }
                names.apply_ini(
                    &IniFile::from_str(&text),
                    row["filename"].as_str().unwrap(),
                    &csf,
                );
            }
            assert_eq!(
                names.name_units(),
                units(&row["name_utf16"]),
                "{}",
                row["name"]
            );
            assert_eq!(
                names.save_title_units(),
                units(&row["save_title_utf16"]),
                "{}",
                row["name"]
            );
            assert_eq!(
                names.ui_name(),
                row["ui_name"].as_str().unwrap(),
                "{}",
                row["name"]
            );
            let bytes = crate::util::save_codec::serialize(&names).unwrap();
            let restored: CampaignScenarioNames =
                crate::util::save_codec::deserialize(&bytes).unwrap();
            assert_eq!(restored, names);
        }
    }

    #[test]
    fn campaign_mission_names_use_physical_mission_and_csf_inputs() {
        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let mission = IniFile::from_bytes(assets.get_ref("MISSIONMD.INI").unwrap()).unwrap();
        let csf =
            crate::assets::csf_file::CsfFile::from_bytes(assets.get_ref("ra2md.csf").unwrap())
                .unwrap();
        for (filename, expected) in [
            ("ALL01UMD.MAP", "Allied 01 - Time Lapse"),
            ("ALL02UMD.MAP", "Allied 02 - Hollywood and Vain"),
            ("SOV01UMD.MAP", "Soviet 01 - Time Shift"),
        ] {
            let mut names = CampaignScenarioNames::default();
            names.apply_ini(&mission, filename, &csf);
            assert_eq!(names.save_title(), expected);
        }
    }

    fn native() -> &'static Value {
        let fixture = crate::test_fixture::json("tools/input_oracle/campaign_start.json");
        fixture
    }

    fn assert_fields(metadata: &CampaignLoadingMetadata, row: &Value) {
        for (key, actual) in [
            ("LSLoadMessage", metadata.load_message_key()),
            ("LSLoadBriefing", metadata.load_briefing_key()),
            ("LS640BkgdName", metadata.background_name_640()),
            ("LS800BkgdName", metadata.background_name_800()),
            ("LS800BkgdPal", metadata.background_palette_name()),
        ] {
            assert_eq!(actual, row["strings"][key].as_str().unwrap(), "{key}");
        }
        let narrow = metadata.brief_location_640();
        let wide = metadata.brief_location_800();
        for (key, actual) in [
            ("LS640BriefLocX", narrow[0]),
            ("LS640BriefLocY", narrow[1]),
            ("LS800BriefLocX", wide[0]),
            ("LS800BriefLocY", wide[1]),
        ] {
            assert_eq!(actual as i64, row["coords"][key].as_i64().unwrap(), "{key}");
        }
    }

    #[test]
    fn mission_loading_fields_match_original_executed_reader() {
        let native = native();
        for row in native["mission_loading"]["rows"].as_array().unwrap() {
            // The executed region's declared prior state; it is deliberately
            // different from SetDefaults so absent-key retention is observed.
            let mut metadata = CampaignLoadingMetadata {
                load_message_key: "Prior".into(),
                load_briefing_key: "Prior".into(),
                brief_location_640: [77, -77],
                brief_location_800: [1234, -1234],
                background_name_640: "Prior".into(),
                background_name_800: "Prior".into(),
                background_palette_name: "Prior".into(),
            };
            let filename = row["filename"].as_str().unwrap();
            let mut text = String::new();
            for (name, section) in row["sections"].as_object().unwrap() {
                text.push_str(&format!("[{name}]\n"));
                for (key, value) in section.as_object().unwrap() {
                    text.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
                }
            }
            metadata.apply_ini(&IniFile::from_str(&text), filename);
            assert_fields(&metadata, row);
        }
    }

    #[test]
    fn retail_restate_briefings_read_their_section_key_or_fall_back_to_the_error_key() {
        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let bytes = assets.get_ref("MISSIONMD.INI").expect("retail MISSIONMD");
        let mission = IniFile::from_bytes(bytes).expect("retail MISSIONMD parses");
        assert_eq!(
            restate_briefing_key(&mission, "ALL01UMD.MAP"),
            "Brief:ALL01md"
        );
        assert_eq!(
            restate_briefing_key(&mission, "SOV07TMD.MAP"),
            "Brief:Sov07md"
        );
        assert_eq!(
            restate_briefing_key(&mission, "C1A01MD.MAP"),
            "NAME:COOPDESC11"
        );
        // A map with no section, such as any skirmish map, shows the error key.
        assert_eq!(restate_briefing_key(&mission, "XMP29U2.MAP"), "Brief:Error");
    }

    #[test]
    fn physical_missionmd_uses_the_production_reader_for_both_first_maps() {
        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let bytes = assets.get_ref("MISSIONMD.INI").expect("retail MISSIONMD");
        let mission = IniFile::from_bytes(bytes).expect("retail MISSIONMD parses");
        let native = native();
        assert_eq!(
            crate::util::sha256::sha256_hex(bytes),
            native["mission_loading"]["missionmd_sha256"]
                .as_str()
                .unwrap(),
            "the physical MISSION input must match the executed corpus"
        );
        for row in native["mission_loading"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| matches!(row["name"].as_str(), Some("stock_all" | "stock_sov")))
        {
            let mut metadata = CampaignLoadingMetadata::new();
            metadata.apply_ini(&mission, row["filename"].as_str().unwrap());
            assert_fields(&metadata, row);
        }
    }
}
