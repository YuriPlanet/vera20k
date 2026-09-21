//! Tiberium type definitions parsed from rules.ini.
//!
//! `[Tiberiums]` defines the native `TiberiumClass` order. Simulation code uses
//! this data as the bridge from overlay cells to per-type growth/spread state.

use std::collections::HashMap;

use crate::rules::ini_parser::{IniFile, IniSection};

/// Native tiberium density byte range is 0..=11.
pub const TIBERIUM_DENSITY_LEVELS: u8 = 12;
const PERCENT_PPM: f64 = 1_000_000.0;

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
    /// Growth timer reload value.
    pub growth: u32,
    /// `GrowthPercentage=` scaled by 1,000,000 at parse time.
    pub growth_percentage_ppm: i32,
    /// `GrowthPercentage=` as the native `TiberiumClass+0xB0` double (IEEE
    /// bits of the correctly rounded decimal); the growth processor multiplies
    /// the heap count by it in x87 arithmetic and compares it against 1e-05.
    pub growth_percentage_bits: u64,
    /// Spread timer reload value.
    pub spread: u32,
    /// `SpreadPercentage=` scaled by 1,000,000 at parse time.
    pub spread_percentage_ppm: i32,
    /// `SpreadPercentage=` as the native `TiberiumClass+0xA0` double.
    pub spread_percentage_bits: u64,
    /// Number of valid overlay data density levels.
    pub max_density: u8,
}

/// Ordered tiberium type registry.
#[derive(Debug, Clone, Default)]
pub struct TiberiumTypeRegistry {
    types: Vec<TiberiumType>,
    by_name: HashMap<String, TiberiumTypeId>,
}

impl TiberiumTypeRegistry {
    pub fn from_ini(ini: &IniFile) -> Self {
        let Some(section) = ini.section("Tiberiums") else {
            return Self::default();
        };

        let mut types = Vec::new();
        let mut by_name = HashMap::new();
        for name in section.get_values() {
            let Some(type_section) = ini.section(name) else {
                continue;
            };
            let Some(id) = u8::try_from(types.len()).ok().map(TiberiumTypeId) else {
                break;
            };
            let ty = TiberiumType::from_ini_section(id, name, type_section);
            by_name.insert(name.to_ascii_uppercase(), id);
            types.push(ty);
        }

        Self { types, by_name }
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
        self.by_name.get(&name.to_ascii_uppercase()).copied()
    }
}

impl TiberiumType {
    fn from_ini_section(id: TiberiumTypeId, section_name: &str, section: &IniSection) -> Self {
        let image = section
            .get_i32("Image")
            .and_then(|v| u8::try_from(v).ok())
            .unwrap_or(1);
        Self {
            id,
            section: section_name.to_string(),
            display_name: section.get("Name").map(str::to_string),
            color: section.get("Color").map(str::to_string),
            image,
            value: section.get_i32("Value").unwrap_or(0),
            growth: section
                .get_i32("Growth")
                .and_then(|v| u32::try_from(v).ok())
                .unwrap_or(0),
            growth_percentage_ppm: section
                .get("GrowthPercentage")
                .and_then(parse_percent_ppm)
                .unwrap_or(0),
            growth_percentage_bits: section
                .get("GrowthPercentage")
                .and_then(parse_percent_bits)
                .unwrap_or(0),
            spread: section
                .get_i32("Spread")
                .and_then(|v| u32::try_from(v).ok())
                .unwrap_or(0),
            spread_percentage_ppm: section
                .get("SpreadPercentage")
                .and_then(parse_percent_ppm)
                .unwrap_or(0),
            spread_percentage_bits: section
                .get("SpreadPercentage")
                .and_then(parse_percent_bits)
                .unwrap_or(0),
            max_density: TIBERIUM_DENSITY_LEVELS,
        }
    }
}

/// The native percentage double as stored by `TiberiumClass::ReadINI`: a
/// correctly rounded decimal parse (the native CRT `atof` rounding of longer
/// decimals is UNCHECKED; retail values are short). An absent key keeps the
/// constructor's zero.
fn parse_percent_bits(raw: &str) -> Option<u64> {
    raw.trim().parse::<f64>().ok().map(f64::to_bits)
}

fn parse_percent_ppm(raw: &str) -> Option<i32> {
    let value = raw.trim().parse::<f64>().ok()?;
    let scaled = (value * PERCENT_PPM).round();
    if scaled < i32::MIN as f64 || scaled > i32::MAX as f64 {
        None
    } else {
        Some(scaled as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(riparius.growth_percentage_ppm, 60_000);
        assert_eq!(riparius.spread, 2200);
        assert_eq!(riparius.spread_percentage_ppm, 60_000);
        assert_eq!(riparius.max_density, TIBERIUM_DENSITY_LEVELS);

        let cruentus = registry
            .get(registry.id_by_name("cruentus").expect("Cruentus id"))
            .expect("Cruentus");
        assert_eq!(cruentus.image, 2);
        assert_eq!(cruentus.value, 50);
        assert_eq!(cruentus.growth, 10000);
        assert_eq!(cruentus.growth_percentage_ppm, 0);
        assert_eq!(cruentus.spread, 10000);
        assert_eq!(cruentus.spread_percentage_ppm, 0);
    }

    #[test]
    fn missing_tiberiums_section_is_empty() {
        let registry = TiberiumTypeRegistry::from_ini(&IniFile::from_str("[General]\n"));

        assert!(registry.is_empty());
    }
}
