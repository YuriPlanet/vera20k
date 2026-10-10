//! Process-retained BATTLEMD campaign definitions.
//!
//! `CampaignClass::ReadList @ 0x0046CE10` appends or reuses definitions;
//! `CampaignClass::ReadINI @ 0x0046CCD0` reads their current fields. The
//! original list survives ClearScene, which rereads the campaign sources.
//! The native corpus is `tools/input_oracle/campaign_start.json`.

use crate::assets::csf_file::CsfFile;
use crate::rules::ini_parser::IniFile;
use crate::rules::movies::MovieRegistry;
use crate::rules::native_processing::abstract_type_stored_id;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CampaignDefinition {
    id: String,
    cd: i32,
    scenario: String,
    final_movie: i32,
    description: String,
}

impl CampaignDefinition {
    fn new(incoming_id: &str) -> Self {
        // Campaign ctor46CB60: CD/final movie -1, empty Scenario/Description.
        Self {
            id: abstract_type_stored_id(incoming_id),
            cd: -1,
            scenario: String::new(),
            final_movie: -1,
            description: String::new(),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn cd(&self) -> i32 {
        self.cd
    }

    pub fn scenario(&self) -> &str {
        &self.scenario
    }

    pub fn final_movie(&self) -> i32 {
        self.final_movie
    }

    pub fn description(&self) -> &str {
        &self.description
    }
}

#[derive(Debug, Default)]
pub struct CampaignRegistry {
    entries: Vec<CampaignDefinition>,
}

impl CampaignRegistry {
    /// Read the source-order list using each entry's original 32-byte buffer.
    /// Lookup compares the full incoming spelling against the stored ID;
    /// a repeated overlong spelling can therefore append equal stored IDs.
    pub fn apply_ini(&mut self, ini: &IniFile, movies: &MovieRegistry, csf: Option<&CsfFile>) {
        let Some(list) = ini.section("Battles") else {
            return;
        };
        for key in list.keys() {
            let incoming = list.read_string(key, "", 32);
            let index = self.find_index(&incoming).map_or_else(
                || {
                    let index = self.entries.len();
                    self.entries.push(CampaignDefinition::new(&incoming));
                    index
                },
                |index| index,
            );
            let entry = &mut self.entries[index];
            // AbstractType::ReadINI410A60 admits only the stored exact-case
            // section. A missing body preserves the constructor/current state.
            let Some(section) = ini.section(&entry.id) else {
                continue;
            };
            entry.cd = section.read_int("CD", entry.cd);
            entry.final_movie = section.read_movie("FinalMovie", entry.final_movie, movies);
            entry.scenario = section.read_string("Scenario", &entry.scenario, 512);
            entry.scenario.make_ascii_uppercase();
            let debug_only = section.read_bool("DebugOnly", false);
            let description = section.read_string("Description", "", 1024);
            entry.description = if debug_only {
                // Native's debug-only byte-widen branch appends this literal
                // unboundedly. String storage avoids its buffer overrun; stock
                // descriptions fit the original 128-wide-character field.
                format!("{description} - for debug/testing")
            } else {
                let localized = csf.map_or_else(
                    || description.clone(),
                    |csf| csf.text(&description).into_owned(),
                );
                String::from_utf16_lossy(&localized.encode_utf16().take(127).collect::<Vec<_>>())
            };
        }
    }

    /// Original CampaignClass::FindIndex46CC90, first case-insensitive ID.
    pub fn find_index(&self, incoming: &str) -> Option<usize> {
        self.entries
            .iter()
            .position(|entry| entry.id.eq_ignore_ascii_case(incoming))
    }

    pub fn get(&self, index: usize) -> Option<&CampaignDefinition> {
        self.entries.get(index)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
