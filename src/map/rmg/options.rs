//! `[RandomMap]` seed/options model, the normalizer, and `.SED` read/write.
//!
//! Field order mirrors the original record so the clamp table and the `.SED`
//! key order stay auditable side by side.
//!
//! `IniFile` lookups here are section-scoped: `ini.section(name)?.get_i32(key)`.
//! There is no `IniFile::get(section, key)` and no `IniFile::parse`; construct
//! with `IniFile::from_bytes` or `IniFile::from_str`.

use super::description::{SeedDescription, read_description};
use crate::rules::ini_parser::IniFile;
use crate::util::ini_writer::set_ini_values;

/// Section a random-map seed file stores its options under.
const SECTION: &str = "RandomMap";

/// One random-map configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RmgOptions {
    pub theater: i32,
    pub map_type: i32,
    pub resources: i32,
    pub ruggedness: i32,
    pub time: i32,
    pub water_amount: i32,
    pub num_players: i32,
    pub tiberium: i32,
    pub tiberium_layout: i32,
    pub vegetation: i32,
    pub urban_presence: i32,
    pub width: i32,
    pub height: i32,
    pub accessibility: i32,
    pub region_size: i32,
    pub seed: i32,
    /// Display description. Stored in `.SED` as comma-separated hex UTF-16 code
    /// units, and used as the random-map row's displayed name.
    pub description: SeedDescription,
}

impl Default for RmgOptions {
    /// The original constructor's defaults. Note `seed = -1`, which the
    /// normalizer later clamps to 0 unless a `.SED` supplies a real seed.
    fn default() -> Self {
        Self {
            theater: 0,
            map_type: 1,
            resources: 1,
            ruggedness: 0,
            time: 1,
            water_amount: 0,
            num_players: 2,
            tiberium: 0,
            tiberium_layout: 0,
            vegetation: 0,
            urban_presence: 0,
            width: 0,
            height: 0,
            accessibility: 0,
            region_size: 0,
            seed: -1,
            description: SeedDescription::default(),
        }
    }
}

/// Encode a description the way the original writes it: each UTF-16 code unit
/// as lowercase hex followed by a comma, including a trailing one.
fn encode_description(text: &SeedDescription) -> String {
    crate::rules::ini_value::encode_comma_hex_utf16(text.units())
}

impl RmgOptions {
    /// Clamp every field to its accepted range.
    ///
    /// Deliberately does **not** touch `theater`: the original normalizer has
    /// no code path that writes that field, so an out-of-range theater survives
    /// and is resolved later by the theater-name lookup.
    pub fn normalize(&mut self) {
        self.resources = self.resources.clamp(0, 3);
        self.map_type = self.map_type.clamp(0, 4);
        self.time = self.time.clamp(0, 3);
        self.ruggedness = self.ruggedness.clamp(0, 100);
        self.water_amount = self.water_amount.clamp(0, 100);
        self.num_players = self.num_players.clamp(2, 8);
        self.tiberium = self.tiberium.clamp(1, 100);
        self.tiberium_layout = self.tiberium_layout.clamp(0, 100);
        self.vegetation = self.vegetation.clamp(0, 100);
        self.urban_presence = self.urban_presence.clamp(0, 100);
        self.width = self.width.clamp(0, 3);
        self.height = self.height.clamp(0, 3);
        self.accessibility = self.accessibility.clamp(0, 100);
        self.region_size = self.region_size.clamp(0, 100);
        self.seed = self.seed.clamp(0, 0xFFFF);
    }

    /// Seed in the form the generator's RNG consumes. Normalizing first
    /// guarantees the value fits.
    pub fn seed_u16(&self) -> u16 {
        self.seed.clamp(0, 0xFFFF) as u16
    }

    /// Apply a `.SED`'s `[RandomMap]` keys over `self`.
    ///
    /// A missing key leaves the existing field alone because the original
    /// passes that field as the reader default. A present malformed value uses
    /// native `atoi` semantics and therefore becomes zero. Does not normalize.
    pub fn apply_sed(&mut self, ini: &IniFile) {
        let Some(section) = ini.section(SECTION) else {
            return;
        };
        if let Some(raw) = section.get("Description") {
            self.description = read_description(Some(raw), &self.description);
        }
        let read = |key: &str, field: &mut i32| {
            if let Some(value) = section.get_i32(key) {
                *field = value;
            }
        };
        read("Width", &mut self.width);
        read("Height", &mut self.height);
        read("NumPlayers", &mut self.num_players);
        read("Seed", &mut self.seed);
        read("MapType", &mut self.map_type);
        read("Theater", &mut self.theater);
        read("Time", &mut self.time);
        read("RegionSize", &mut self.region_size);
        read("Ruggedness", &mut self.ruggedness);
        read("Accessibility", &mut self.accessibility);
        read("WaterAmount", &mut self.water_amount);
        read("Tiberium", &mut self.tiberium);
        read("TiberiumLayout", &mut self.tiberium_layout);
        read("Vegetation", &mut self.vegetation);
        read("UrbanPresence", &mut self.urban_presence);
        read("Resources", &mut self.resources);
    }

    /// Serialize to `.SED` bytes, writing every key in the original's order.
    pub fn to_sed_bytes(&self) -> Vec<u8> {
        let values: Vec<(&str, String)> = vec![
            // The original emits Description first, ahead of the 16 integers.
            ("Description", encode_description(&self.description)),
            ("Width", self.width.to_string()),
            ("Height", self.height.to_string()),
            ("NumPlayers", self.num_players.to_string()),
            ("Seed", self.seed.to_string()),
            ("MapType", self.map_type.to_string()),
            ("Theater", self.theater.to_string()),
            ("Time", self.time.to_string()),
            ("RegionSize", self.region_size.to_string()),
            ("Ruggedness", self.ruggedness.to_string()),
            ("Accessibility", self.accessibility.to_string()),
            ("WaterAmount", self.water_amount.to_string()),
            ("Tiberium", self.tiberium.to_string()),
            ("TiberiumLayout", self.tiberium_layout.to_string()),
            ("Vegetation", self.vegetation.to_string()),
            ("UrbanPresence", self.urban_presence.to_string()),
            ("Resources", self.resources.to_string()),
        ];
        let pairs: Vec<(&str, &str)> = values
            .iter()
            .map(|(key, value)| (*key, value.as_str()))
            .collect();
        set_ini_values(b"", SECTION, &pairs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theater_is_never_clamped() {
        let mut high = RmgOptions {
            theater: 99,
            ..Default::default()
        };
        high.normalize();
        assert_eq!(high.theater, 99, "the normalizer never writes theater");

        let mut negative = RmgOptions {
            theater: -5,
            ..Default::default()
        };
        negative.normalize();
        assert_eq!(negative.theater, -5);
    }

    #[test]
    fn clamp_bounds_match_the_original() {
        let mut options = RmgOptions {
            theater: 0,
            resources: 9,
            map_type: 9,
            time: 9,
            ruggedness: 500,
            water_amount: -1,
            num_players: 1,
            tiberium: 0,
            tiberium_layout: 500,
            vegetation: -3,
            urban_presence: 900,
            width: 7,
            height: 7,
            accessibility: 101,
            region_size: -1,
            seed: 0x1_0000,
            description: SeedDescription::default(),
        };
        options.normalize();

        assert_eq!(
            (options.resources, options.map_type, options.time),
            (3, 4, 3)
        );
        assert_eq!((options.ruggedness, options.water_amount), (100, 0));
        assert_eq!(options.num_players, 2, "player floor is 2, not 0");
        assert_eq!(options.tiberium, 1, "tiberium floor is 1, not 0");
        assert_eq!(
            (
                options.tiberium_layout,
                options.vegetation,
                options.urban_presence
            ),
            (100, 0, 100)
        );
        assert_eq!((options.width, options.height), (3, 3));
        assert_eq!((options.accessibility, options.region_size), (100, 0));
        assert_eq!(options.seed, 0xFFFF);
    }

    #[test]
    fn defaults_match_the_original_constructor() {
        let options = RmgOptions::default();
        assert_eq!(options.map_type, 1);
        assert_eq!(options.resources, 1);
        assert_eq!(options.time, 1);
        assert_eq!(options.num_players, 2);
        assert_eq!(options.theater, 0);
        assert_eq!(options.seed, -1, "unset seed is -1 before normalizing");
    }

    #[test]
    fn missing_keys_carry_existing_values() {
        let mut options = RmgOptions {
            ruggedness: 42,
            num_players: 6,
            ..Default::default()
        };
        options.apply_sed(&IniFile::from_str("[RandomMap]\nSeed=7\n"));

        assert_eq!(options.seed, 7, "present key applies");
        assert_eq!(options.ruggedness, 42, "absent key carries, never zeroes");
        assert_eq!(options.num_players, 6);
    }

    #[test]
    fn malformed_value_uses_native_atoi_zero() {
        let mut options = RmgOptions {
            tiberium: 55,
            ..Default::default()
        };
        options.apply_sed(&IniFile::from_str("[RandomMap]\nTiberium=abc\n"));
        assert_eq!(options.tiberium, 0);
    }

    #[test]
    fn missing_section_is_a_no_op() {
        let mut options = RmgOptions {
            seed: 123,
            ..Default::default()
        };
        options.apply_sed(&IniFile::from_str("[Basic]\nName=nope\n"));
        assert_eq!(options.seed, 123);
    }

    #[test]
    fn sed_round_trips() {
        let mut original = RmgOptions {
            theater: 2,
            map_type: 3,
            resources: 2,
            ruggedness: 40,
            time: 1,
            water_amount: 55,
            num_players: 6,
            tiberium: 30,
            tiberium_layout: 20,
            vegetation: 70,
            urban_presence: 10,
            width: 2,
            height: 3,
            accessibility: 60,
            region_size: 45,
            seed: 4321,
            description: "Round Trip".into(),
        };
        original.normalize();

        let bytes = original.to_sed_bytes();
        let mut parsed = RmgOptions::default();
        parsed.apply_sed(&IniFile::from_bytes(&bytes).unwrap());
        parsed.normalize();

        assert_eq!(parsed, original);
    }

    #[test]
    fn seed_u16_survives_the_unset_sentinel() {
        let options = RmgOptions::default();
        assert_eq!(options.seed, -1);
        assert_eq!(options.seed_u16(), 0, "-1 must not wrap to 0xFFFF");
    }

    #[test]
    fn description_encodes_to_the_native_comma_hex_form() {
        // The trailing comma is not a typo: the original appends the delimiter
        // after every code unit, including the last.
        assert_eq!(
            encode_description(&"Random Map".into()),
            "52,61,6e,64,6f,6d,20,4d,61,70,"
        );
    }

    #[test]
    fn description_decodes_the_native_form() {
        assert_eq!(
            read_description(
                Some("52,61,6e,64,6f,6d,20,4d,61,70,"),
                &SeedDescription::default()
            ),
            "Random Map"
        );
    }

    #[test]
    fn description_round_trips_through_sed() {
        let mut original = RmgOptions {
            description: "Random Map".into(),
            seed: 1234,
            ..Default::default()
        };
        original.normalize();

        let bytes = original.to_sed_bytes();
        let mut parsed = RmgOptions::default();
        parsed.apply_sed(&IniFile::from_bytes(&bytes).unwrap());
        parsed.normalize();

        assert_eq!(parsed.description, "Random Map");
        assert_eq!(parsed, original);
    }

    #[test]
    fn description_is_the_first_emitted_key() {
        let bytes = RmgOptions::default().to_sed_bytes();
        let text = String::from_utf8(bytes).expect("ini is utf-8");
        let body = text.split("[RandomMap]").nth(1).expect("section present");
        let first = body
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("");
        assert!(first.starts_with("Description"), "got {first:?}");
    }

    #[test]
    fn malformed_description_tokens_repeat_the_previous_conversion() {
        assert_eq!(
            read_description(Some("52,zz,61,"), &SeedDescription::default()),
            "RRa"
        );
    }

    #[test]
    fn seed_persistence_preserves_unpaired_utf16_units() {
        // NewEdit Delete/Backspace works in UTF-16 units and can leave a high
        // surrogate without its low half. Display conversion is not storage.
        let original = RmgOptions {
            description: SeedDescription::from_units([0xd83d, 0x41]),
            ..RmgOptions::default()
        };
        assert_eq!(original.description.display_text(), "\u{fffd}A");
        let bytes = original.to_sed_bytes();
        let mut loaded = RmgOptions::default();
        loaded.apply_sed(&IniFile::from_bytes(&bytes).unwrap());
        assert_eq!(loaded.description.units(), [0xd83d, 0x41]);
        assert_eq!(loaded.description, original.description);
        assert!(
            String::from_utf8(bytes)
                .unwrap()
                .contains("Description=d83d,41,")
        );
    }
}
