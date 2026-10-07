//! Tiberium type definitions parsed from rules.ini.
//!
//! `[Tiberiums]` defines the native `TiberiumClass` order. Simulation code uses
//! this data as the bridge from overlay cells to per-type growth/spread state.

use crate::rules::ini_parser::{IniFile, IniSection};

/// Native tiberium density byte range is 0..=11.
pub const TIBERIUM_DENSITY_LEVELS: u8 = 12;
/// `TiberiumClass::Constructor @ 0x007216C0` seeds both percentages with
/// `0x3FB999999999999A` (0.1) at `0x007216DC`/`0x007216E7`.
const CTOR_PERCENTAGE: f64 = 0.1;

/// Index into the parsed `[Tiberiums]` list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TiberiumTypeId(pub u8);

/// Per-`TiberiumClass` rules data needed by growth/spread and placement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TiberiumType {
    pub id: TiberiumTypeId,
    pub section: String,
    pub display_name: Option<String>,
    /// `[TiberiumType] Color=` selector for the ConvertClass installed on a
    /// CellAnim created over this resource.
    pub color: Option<String>,
    /// `Image=` selector: 1 = TIB, 2 = GEM, 3 = TIB2, 4 = TIB3.
    pub image: u8,
    /// Credit value per harvested density unit.
    pub value: i32,
    /// Signed `Growth=` timer reload input (`TiberiumClass+0xA8`).
    pub growth: i32,
    /// `GrowthPercentage=` as the native `TiberiumClass+0xB0` double (IEEE
    /// bits); the growth processor multiplies the heap count by it in x87
    /// arithmetic and compares it against 1e-05.
    pub growth_percentage_bits: u64,
    /// Signed `Spread=` timer reload value (`TiberiumClass+0x9C`).
    pub spread: i32,
    /// `SpreadPercentage=` as the native `TiberiumClass+0xA0` double.
    pub spread_percentage_bits: u64,
    /// Number of valid overlay data density levels.
    pub max_density: u8,
}

/// Ordered tiberium type registry.
#[derive(Debug, Clone, Default)]
pub struct TiberiumTypeRegistry {
    types: Vec<TiberiumType>,
}

impl TiberiumTypeRegistry {
    pub fn from_ini(ini: &IniFile) -> Self {
        let Some(section) = ini.section("Tiberiums") else {
            return Self::default();
        };

        let mut types = Vec::new();
        for name in section.registry_ids() {
            let Some(type_section) = ini.section(name) else {
                continue;
            };
            let Some(id) = u8::try_from(types.len()).ok().map(TiberiumTypeId) else {
                break;
            };
            types.push(TiberiumType::from_ini_section(id, name, type_section));
        }

        Self { types }
    }

    pub fn len(&self) -> usize {
        self.types.len()
    }

    pub fn is_empty(&self) -> bool {
        self.types.is_empty()
    }

    pub fn types(&self) -> &[TiberiumType] {
        &self.types
    }

    pub fn get(&self, id: TiberiumTypeId) -> Option<&TiberiumType> {
        self.types.get(id.0 as usize)
    }

    #[cfg(test)]
    pub fn id_by_name(&self, name: &str) -> Option<TiberiumTypeId> {
        self.types
            .iter()
            .find(|ty| ty.section.eq_ignore_ascii_case(name))
            .map(|ty| ty.id)
    }
}

impl TiberiumType {
    /// `TiberiumClass::ReadINI @ 0x00721A50`: every read passes the current
    /// field, which starts at the constructor's value.
    fn from_ini_section(id: TiberiumTypeId, section_name: &str, section: &IniSection) -> Self {
        // `0x00721C49` ReadInt(-1) selects the overlay run: 2, 3 and 4 pick
        // their own, every other value Riparius. RESIDUAL: -1 (and an absent
        // key) keeps the constructor's null run natively; VERA reads it as
        // Riparius. Every retail type authors 1-4.
        let image = u8::try_from(section.read_int("Image", -1)).unwrap_or(1);
        Self {
            id,
            section: section_name.to_string(),
            // AbstractTypeClass `Name=`, ReadString 0x31 (`0x00410AA0`).
            display_name: section.read_name("Name", 0x31).map(str::to_string),
            // `0x00721B35`: ReadString 0x20, then the color scheme lookup.
            color: section.read_name("Color", 0x20).map(str::to_string),
            image,
            value: section.read_int("Value", 0),
            // ReadInt stores its signed result directly, without a clamp
            // (`0x00721AE1`); original execution: spatial_oracle/ore_queue.md.
            growth: section.read_int("Growth", 0),
            // ReadDouble -> `FSTP qword [ESI+0xB0]` (`0x00721AF4`).
            growth_percentage_bits: section
                .read_double("GrowthPercentage", CTOR_PERCENTAGE)
                .to_bits(),
            // The same raw signed store at `0x00721AA6`.
            spread: section.read_int("Spread", 0),
            // ReadDouble -> `FSTP qword [ESI+0xA0]` (`0x00721AB9`).
            spread_percentage_bits: section
                .read_double("SpreadPercentage", CTOR_PERCENTAGE)
                .to_bits(),
            max_density: TIBERIUM_DENSITY_LEVELS,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Selected original constructor/ReadINI instructions and physical RULESMD
    // inputs; executable identity and fixture bounds: ore_queue.md.
    #[test]
    fn timer_and_percentage_readers_match_original_constructor_and_read_ini() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/ore_queue.json",
        ))
        .unwrap();
        let rows = corpus["timer_reader_cases"].as_array().unwrap();
        assert_eq!(rows.len(), 16);
        let retail = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini");
        for row in rows {
            let name = row["name"].as_str().unwrap();
            let mut text = String::from("[Tiberiums]\n0=Riparius\n[Riparius]\nImage=1\n");
            for (field, key) in [
                ("growth", "Growth"),
                ("spread", "Spread"),
                ("growth_percentage", "GrowthPercentage"),
                ("spread_percentage", "SpreadPercentage"),
            ] {
                if let Some(value) = row["input"][field].as_str() {
                    text.push_str(&format!("{key}={value}\n"));
                }
            }
            let registry = TiberiumTypeRegistry::from_ini(&IniFile::from_str(&text));
            assert_native_timer_inputs(registry.get(TiberiumTypeId(0)).unwrap(), row, name);
            if let (Some(retail), Some(section)) = (&retail, row["physical"]["section"].as_str()) {
                let registry = TiberiumTypeRegistry::from_ini(retail);
                let ty = registry.get(registry.id_by_name(section).unwrap()).unwrap();
                assert_native_timer_inputs(ty, row, name);
            }
        }
    }

    fn assert_native_timer_inputs(ty: &TiberiumType, row: &serde_json::Value, name: &str) {
        for (field, actual) in [("growth", ty.growth), ("spread", ty.spread)] {
            assert_eq!(
                i64::from(actual),
                row["output"][field].as_i64().unwrap(),
                "{name}: {field}"
            );
        }
        for (field, actual) in [
            ("growth_percentage_bits", ty.growth_percentage_bits),
            ("spread_percentage_bits", ty.spread_percentage_bits),
        ] {
            let expected = u64::from_str_radix(row["output"][field].as_str().unwrap(), 16).unwrap();
            assert_eq!(actual, expected, "{name}: {field}");
        }
    }

    #[test]
    fn parses_tiberiums_order_and_per_type_growth_spread_fields() {
        let ini = IniFile::from_str(
            "\
[Tiberiums]
0=Riparius
1=Cruentus
2=Vinifera
3=Aboreus

[Riparius]
Name=Tiberium Riparius
Color=NeonGreen
Image=1
Value=25
Growth=2200
GrowthPercentage=.06
Spread=2200
SpreadPercentage=.06

[Cruentus]
Name=Tiberium Cruentus
Image=2
Value=50
Growth=10000
GrowthPercentage=0
Spread=10000
SpreadPercentage=0

[Vinifera]
Image=3
Value=25
Growth=2200
GrowthPercentage=.06
Spread=2200
SpreadPercentage=.06

[Aboreus]
Image=4
Value=25
Growth=2200
GrowthPercentage=.06
Spread=2200
SpreadPercentage=.06
",
        );

        let registry = TiberiumTypeRegistry::from_ini(&ini);

        assert_eq!(registry.len(), 4);
        let riparius = registry.get(TiberiumTypeId(0)).expect("Riparius");
        assert_eq!(riparius.section, "Riparius");
        assert_eq!(riparius.color.as_deref(), Some("NeonGreen"));
        assert_eq!(riparius.image, 1);
        assert_eq!(riparius.value, 25);
        assert_eq!(riparius.growth, 2200);
        // ReadDouble scans a float: `.06` is `0.06f32` widened.
        let widened = f64::from(0.06_f32).to_bits();
        assert_eq!(riparius.growth_percentage_bits, widened);
        assert_eq!(riparius.spread, 2200);
        assert_eq!(riparius.spread_percentage_bits, widened);
        assert_eq!(riparius.max_density, TIBERIUM_DENSITY_LEVELS);

        let cruentus = registry
            .get(registry.id_by_name("cruentus").expect("Cruentus id"))
            .expect("Cruentus");
        assert_eq!(cruentus.image, 2);
        assert_eq!(cruentus.value, 50);
        assert_eq!(cruentus.growth, 10000);
        assert_eq!(cruentus.growth_percentage_bits, 0.0_f64.to_bits());
        assert_eq!(cruentus.spread, 10000);
        assert_eq!(cruentus.spread_percentage_bits, 0.0_f64.to_bits());
    }

    #[test]
    fn missing_tiberiums_section_is_empty() {
        let registry = TiberiumTypeRegistry::from_ini(&IniFile::from_str("[General]\n"));

        assert!(registry.is_empty());
    }
}
