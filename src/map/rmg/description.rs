//! Fresh-file MapSeed Description decoding.
//!
//! Native identity: INIClass__ReadCommaHexUTF16 0x00528F00, called first on a
//! freshly loaded INI by MapSeed Load 0x00597A30 and metadata 0x00597D60.
//! See docs/research/skirmish-ui/2026-09-12-seed-description-reader.md.

use crate::rules::ini_parser::IniSection;

const DESCRIPTION_UNITS: usize = 128;

/// Native wide text, including unpaired surrogate units created by NewEdit's
/// one-unit Backspace/Delete. Display conversion must not rewrite stored data.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeedDescription(Vec<u16>);

impl SeedDescription {
    pub fn from_units(units: impl IntoIterator<Item = u16>) -> Self {
        Self(units.into_iter().take_while(|unit| *unit != 0).collect())
    }

    pub fn units(&self) -> &[u16] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn display_text(&self) -> String {
        String::from_utf16_lossy(&self.0)
    }
}

impl From<&str> for SeedDescription {
    fn from(text: &str) -> Self {
        Self::from_units(text.encode_utf16())
    }
}

impl From<String> for SeedDescription {
    fn from(text: String) -> Self {
        Self::from(text.as_str())
    }
}

impl PartialEq<&str> for SeedDescription {
    fn eq(&self, other: &&str) -> bool {
        self.0.iter().copied().eq(other.encode_utf16())
    }
}

/// `[RandomMap] Description` through [`IniSection::read_comma_hex_utf16`],
/// keeping unpaired surrogate units. On the fresh-file section-pointer cache
/// miss a failed first conversion emits the low half of the `RandomMap`
/// section CRC (0xB573).
pub(super) fn read_description(section: &IniSection, default: &SeedDescription) -> SeedDescription {
    SeedDescription(section.read_comma_hex_utf16("Description", default.units(), DESCRIPTION_UNITS))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn random_map(raw: Option<&str>) -> IniSection {
        let mut section = IniSection::new("RandomMap".to_string());
        if let Some(raw) = raw {
            section.set("Description", raw);
        }
        section
    }

    #[test]
    fn fresh_seed_descriptions_match_original_reader_vectors() {
        let vectors: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/sed_description.json",
        ))
        .unwrap();
        let cases = vectors["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 29);
        let mut compared = 0;
        for case in cases {
            if case["cached_section_pointer"].as_bool().unwrap() {
                assert_eq!(case["name"], "cached_section_diagnostic");
                continue;
            }
            assert_eq!(case["count"], 128);
            let expected: Vec<u16> = case["visible_units"]
                .as_array()
                .unwrap()
                .iter()
                .map(|unit| u16::try_from(unit.as_u64().unwrap()).unwrap())
                .collect();
            let section = random_map(case["raw"].as_str());
            assert_eq!(
                read_description(&section, &"DEFAULT".into()).units(),
                expected,
                "{}",
                case["name"]
            );
            compared += 1;
        }
        assert_eq!(compared, 28);
        assert_eq!(
            read_description(&random_map(None), &"Default map".into()),
            "Default map"
        );
    }
}
