//! Process-retained BATTLEMD campaign definitions.
//!
//! `CampaignClass::ReadList @ 0x0046CE10` appends or reuses definitions;
//! `CampaignClass::ReadINI @ 0x0046CCD0` reads their current fields. The
//! original list survives ClearScene, which rereads the campaign sources.
//! The native corpus is `tools/input_oracle/campaign_start.json`.
//! `DebugOnly=` (`0x0046CD5E`) and `Description=` (`0x0046CD7B`, or
//! `0x0046CDA3` when DebugOnly) are not read: nothing displays a campaign
//! description yet.

use crate::rules::ini_parser::IniFile;
use crate::rules::movies::MovieRegistry;
use crate::rules::native_processing::abstract_type_stored_id;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CampaignDefinition {
    id: String,
    cd: i32,
    scenario: String,
    final_movie: i32,
}

impl CampaignDefinition {
    fn new(incoming_id: &str) -> Self {
        // Campaign ctor46CB60: CD/final movie -1, empty Scenario.
        Self {
            id: abstract_type_stored_id(incoming_id),
            cd: -1,
            scenario: String::new(),
            final_movie: -1,
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
}

#[derive(Debug, Default)]
pub struct CampaignRegistry {
    entries: Vec<CampaignDefinition>,
}

impl CampaignRegistry {
    /// Read the source-order list using each entry's original 32-byte buffer.
    /// Lookup compares the full incoming spelling against the stored ID;
    /// a repeated overlong spelling can therefore append equal stored IDs.
    pub fn apply_ini(&mut self, ini: &IniFile, movies: &MovieRegistry) {
        let Some(list) = ini.section("Battles") else {
            return;
        };
        for key in list.keys() {
            let incoming = list.read_string(key, "", 32);
            let index = self.find_index(&incoming).unwrap_or_else(|| {
                let index = self.entries.len();
                self.entries.push(CampaignDefinition::new(&incoming));
                index
            });
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

    /// The definitions in `[Battles]` list order.
    pub fn iter(&self) -> impl Iterator<Item = &CampaignDefinition> {
        self.entries.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
