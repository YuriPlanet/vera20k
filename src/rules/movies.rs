//! The process movie registry selected from ARTMD.INI.
//!
//! `Load_Game_Rules @ 0x0052D121` calls `Read_Movies @ 0x00674550`
//! with the fixed ART INI. Scenario Rules passes do not replace this list.
//! `Movie_From_Name @ 0x0048DF30` searches the first matching entry using
//! ASCII case-insensitive comparison. Native execution coverage is recorded
//! in `tools/input_oracle/campaign_start.meta.json`.

use crate::rules::ini_parser::IniFile;

#[derive(Debug, Default)]
pub struct MovieRegistry {
    names: Vec<String>,
}

impl MovieRegistry {
    pub(crate) fn from_art(art: &IniFile) -> Self {
        let mut registry = Self::default();
        if let Some(section) = art.section("Movies") {
            for key in section.keys() {
                let name = section.read_string(key, "<none>", 32);
                // Read_Movies674550 only looks up/adds a name when the
                // original ReadString returns nonzero. Stored-empty cache
                // controls therefore cannot allocate an empty catalog slot.
                if !name.is_empty() && registry.find_index(&name) == -1 {
                    registry.names.push(name);
                }
            }
        }
        registry
    }

    /// Native `Movie_From_Name @ 0x0048DF30`; no allocation on an unknown name.
    pub fn find_index(&self, name: &str) -> i32 {
        // This reader treats only `<none>` as its sentinel. The bare name
        // `none` can be an ordinary catalog entry (unlike ReadType callers).
        if name.is_empty() || name.eq_ignore_ascii_case("<none>") {
            return -1;
        }
        self.names
            .iter()
            .position(|entry| entry.eq_ignore_ascii_case(name))
            .map_or(-1, |index| index as i32)
    }

    pub fn name(&self, index: i32) -> Option<&str> {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.names.get(index))
            .map(String::as_str)
    }
}
