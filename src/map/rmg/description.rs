//! Fresh-file MapSeed Description decoding.
//!
//! Native identity: INIClass__ReadCommaHexUTF16 0x00528F00, called first on a
//! freshly loaded INI by MapSeed Load 0x00597A30 and metadata 0x00597D60.
//! See docs/research/skirmish-ui/2026-09-12-seed-description-reader.md.

const DESCRIPTION_UNITS: usize = 128;
// On a section-pointer cache miss, 0x00529023 leaves this native CRC in the
// sscanf destination. An initial failed conversion therefore emits 0xB573.
const RANDOM_MAP_SECTION_CRC: u32 = 0x1597_b573;

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

/// Decode the visible UTF-16 units without changing unpaired surrogates.
fn decode_units(raw: Option<&str>, default: &[u16]) -> Vec<u16> {
    crate::rules::ini_value::read_comma_hex_utf16(
        raw,
        default,
        DESCRIPTION_UNITS,
        RANDOM_MAP_SECTION_CRC,
    )
}

pub(super) fn read_description(raw: Option<&str>, default: &SeedDescription) -> SeedDescription {
    SeedDescription(decode_units(raw, default.units()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_map_section_crc_is_the_crc_engine_of_its_name() {
        assert_eq!(
            crate::assets::mix_hash::crc_engine(b"RandomMap"),
            RANDOM_MAP_SECTION_CRC
        );
    }

    #[test]
    fn fresh_seed_descriptions_match_original_reader_vectors() {
        let vectors: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/storage_oracle/sed_description.json"
        ))
        .unwrap();
        let cases = vectors["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 29);
        let default: Vec<u16> = "DEFAULT".encode_utf16().collect();
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
            let raw = case["raw"].as_str();
            assert_eq!(decode_units(raw, &default), expected, "{}", case["name"]);
            assert_eq!(read_description(raw, &"DEFAULT".into()).units(), expected);
            compared += 1;
        }
        assert_eq!(compared, 28);
        assert_eq!(read_description(None, &"Default map".into()), "Default map");
    }
}
