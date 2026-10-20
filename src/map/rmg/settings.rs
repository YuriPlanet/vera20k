//! `RMGMD.INI [General]` settings for the map generator.
//!
//! The file ships inside the retail MIX archives. When a key is missing the
//! original keeps whatever its constructor left in the field — and those
//! defaults are *not* zero, so an absent file still produces trees and ore.

use crate::assets::asset_manager::AssetManager;
use crate::rules::ini_parser::{IniFile, IniSection};

/// Constructor defaults, used when `RMGMD.INI` (or an individual key) is absent.
const DEFAULT_MIN_TIBERIUM: i32 = 2500;
const DEFAULT_MAX_TIBERIUM: i32 = 5500;
const DEFAULT_MAX_TREES: i32 = 500;

/// Time-of-day buckets: morning, day, dusk, night.
const TIME_BUCKETS: usize = 4;
/// Map-type buckets: archipelago, continent, team continent, inland, mountainous.
const MAP_TYPE_BUCKETS: usize = 5;

/// Generator tuning read from `RMGMD.INI`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RmgSettings {
    pub min_tiberium: i32,
    pub max_tiberium: i32,
    pub max_trees: i32,
    /// Indexed by time of day.
    pub level_light: [i32; TIME_BUCKETS],
    /// Indexed by map type.
    pub vegetation_min: [i32; MAP_TYPE_BUCKETS],
    pub vegetation_max: [i32; MAP_TYPE_BUCKETS],
}

impl Default for RmgSettings {
    fn default() -> Self {
        Self {
            min_tiberium: DEFAULT_MIN_TIBERIUM,
            max_tiberium: DEFAULT_MAX_TIBERIUM,
            max_trees: DEFAULT_MAX_TREES,
            level_light: [0; TIME_BUCKETS],
            vegetation_min: [0; MAP_TYPE_BUCKETS],
            vegetation_max: [0; MAP_TYPE_BUCKETS],
        }
    }
}

impl RmgSettings {
    /// Load from the asset manager, falling back per key to the defaults above.
    pub fn load(assets: &AssetManager) -> Self {
        let mut settings = Self::default();
        let Some(bytes) = assets
            .get_ref("rmgmd.ini")
            .or_else(|| assets.get_ref("rmg.ini"))
        else {
            return settings;
        };
        let Ok(ini) = IniFile::from_bytes(bytes) else {
            return settings;
        };
        settings.apply(&ini);
        settings
    }

    /// `[General]` through ReadInt over each field and the IntVector reader
    /// (`0x00475D70`) for the lists (`0x005982D0..0x0059877A`).
    fn apply(&mut self, ini: &IniFile) {
        let general = ini.section_or_empty("General");
        self.min_tiberium = general.read_int("RMGMinimumTiberium", self.min_tiberium);
        self.max_tiberium = general.read_int("RMGMaximumTiberium", self.max_tiberium);
        self.max_trees = general.read_int("MaxTrees", self.max_trees);
        if let Some(value) = int_list::<TIME_BUCKETS>(general, "RMGLevelLightSettings") {
            self.level_light = value;
        }
        if let Some(value) = int_list::<MAP_TYPE_BUCKETS>(general, "RMGVegetationMinimums") {
            self.vegetation_min = value;
        }
        if let Some(value) = int_list::<MAP_TYPE_BUCKETS>(general, "RMGVegetationMaximums") {
            self.vegetation_max = value;
        }
    }
}

/// Native keeps the whole vector and indexes it by bucket. Rust holds fixed
/// arrays, so a list shorter than its bucket count keeps the default rather
/// than letting a lookup run past the vector.
fn int_list<const N: usize>(general: &IniSection, key: &str) -> Option<[i32; N]> {
    let parsed = general.read_int_list(key)?;
    (parsed.len() >= N).then(|| std::array::from_fn(|i| parsed[i]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_file_uses_constructor_defaults() {
        let settings = RmgSettings::default();
        assert_eq!(settings.min_tiberium, 2500);
        assert_eq!(settings.max_tiberium, 5500);
        assert_eq!(
            settings.max_trees, 500,
            "an absent RMGMD.INI must not mean zero trees"
        );
    }

    #[test]
    fn parses_retail_values() {
        // Verbatim from the RMGMD.INI shipped in the retail archives.
        let ini = IniFile::from_str(
            "[General]\n\
             RMGMinimumTiberium=900\n\
             RMGMaximumTiberium=1050\n\
             RMGLevelLightSettings=3,3,3,3\n\
             RMGVegetationMinimums=60,60,60,60,60\n\
             RMGVegetationMaximums=100,100,100,100,100\n\
             MaxTrees=600\n",
        );
        let mut settings = RmgSettings::default();
        settings.apply(&ini);

        assert_eq!((settings.min_tiberium, settings.max_tiberium), (900, 1050));
        assert_eq!(settings.max_trees, 600);
        assert_eq!(settings.level_light, [3, 3, 3, 3]);
        assert_eq!(settings.vegetation_min, [60; 5]);
        assert_eq!(settings.vegetation_max, [100; 5]);
    }

    #[test]
    fn partial_file_keeps_defaults_for_missing_keys() {
        let ini = IniFile::from_str("[General]\nMaxTrees=42\n");
        let mut settings = RmgSettings::default();
        settings.apply(&ini);

        assert_eq!(settings.max_trees, 42);
        assert_eq!(
            settings.min_tiberium, 2500,
            "an unlisted key keeps its default"
        );
    }

    #[test]
    fn short_list_falls_back_rather_than_partially_applying() {
        let ini = IniFile::from_str("[General]\nRMGVegetationMinimums=60,60\n");
        let mut settings = RmgSettings::default();
        settings.apply(&ini);
        assert_eq!(
            settings.vegetation_min, [0; 5],
            "a truncated list must not leave half-written state"
        );
    }
}
