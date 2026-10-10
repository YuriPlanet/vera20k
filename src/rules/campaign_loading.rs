//! Scenario-owned campaign loading fields read from `MISSIONMD.INI`.
//!
//! `ScenarioClass::Full_Init @ 0x00687000..0x006873AB` reads the exact
//! `Scenario.FileName` section on each admitted MISSION pass. Strings are
//! cleared before their presence gates; coordinates use the current field as
//! their default. `SetDefaults @ 0x006839FE..0x00683A2E` initializes both to
//! empty/zero. Native execution is retained in
//! `tools/input_oracle/campaign_start.{json,meta.json}`.

use crate::rules::ini_parser::IniFile;

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
    use super::CampaignLoadingMetadata;
    use crate::rules::ini_parser::IniFile;
    use serde_json::Value;

    fn native() -> Value {
        let fixture: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/input_oracle/campaign_start.json",
        ))
        .expect("original campaign-start corpus");
        assert_eq!(
            fixture["native_sha256"],
            "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
        );
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
