//! `RMGMD.INI [General]` settings for the map generator.
//!
//! The file ships inside the retail MIX archives. When a key is missing the
//! original keeps whatever its constructor left in the field — and those
//! defaults are *not* zero, so an absent file still produces trees and ore.

use crate::assets::asset_manager::AssetManager;
use crate::rules::ini_parser::{IniFile, IniSection};

use super::{RmgOptions, x87::TruncF64};

/// Constructor defaults, used when `RMGMD.INI` (or an individual key) is absent.
const DEFAULT_MIN_TIBERIUM: i32 = 2500;
const DEFAULT_MAX_TIBERIUM: i32 = 5500;
const DEFAULT_MAX_TREES: i32 = 500;

/// Map-type buckets: archipelago, continent, team continent, inland, mountainous.
const MAP_TYPE_BUCKETS: usize = 5;

/// Generator tuning read from `RMGMD.INI`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RmgSettings {
    pub min_tiberium: i32,
    pub max_tiberium: i32,
    pub max_trees: i32,
    /// Indexed by time of day.
    pub level_light: Vec<i32>,
    pub temperate_ambient: Vec<i32>,
    pub snow_ambient: Vec<i32>,
    /// Red, green, blue vectors, indexed by time of day.
    pub temperate_rgb: [Vec<i32>; 3],
    pub snow_rgb: [Vec<i32>; 3],
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
            level_light: Vec::new(),
            temperate_ambient: Vec::new(),
            snow_ambient: Vec::new(),
            temperate_rgb: Default::default(),
            snow_rgb: Default::default(),
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

    /// Synthetic accepted-map Lighting from InitMapFromSyntheticINI (00599863..
    /// 005999DC). Live preview uses a separate profile after SetDefaults.
    ///
    /// Native indexes the selected vector entries without a length guard. Missing
    /// entries have no defined numeric fallback; preserve the existing empty INI
    /// for that unsupported input rather than inventing ambient values.
    pub(super) fn generated_lighting_ini(&self, options: &RmgOptions) -> Option<IniFile> {
        let time = usize::try_from(options.time).ok()?;
        let ambient = if options.theater == 0 {
            &self.temperate_ambient
        } else {
            &self.snow_ambient
        };
        let ambient = TruncF64::from_f64(f64::from(*ambient.get(time)?))
            .mul(TruncF64::from_f64(0.01))
            .to_f64();
        // 0059991D..0059993D divides signed integers before FILD, not /100.0.
        let level = f64::from(*self.level_light.get(time)? / 100);
        // WriteDouble (005285B0) uses sprintf("%f"): fixed six decimals.
        Some(IniFile::from_str(&format!(
            "[Lighting]\nAmbient={ambient:.6}\nRedTint=1.000000\n\
             GreenTint=1.000000\nBlueTint=1.000000\nGround=0.000000\n\
             Level={level:.6}\nIonAmbient={ambient:.6}\nIonRed=0.300000\n\
             IonGreen=0.400000\nIonBlue=0.750000\nIonGround=0.000000\nIonLevel=0.000000\n"
        )))
    }

    /// InitMap's common tail (0059A529..0059A5EC) follows synthetic Full_Init.
    /// It overwrites configured ambient, raw Level and RGB, not current/target
    /// ambient, Ground or the alternate profiles.
    pub(super) fn generated_lighting_override(
        &self,
        options: &RmgOptions,
    ) -> Option<crate::map::construction_trace::GeneratedLightingOverride> {
        let time = usize::try_from(options.time).ok()?;
        let (ambient, rgb) = if options.theater == 0 {
            (&self.temperate_ambient, &self.temperate_rgb)
        } else {
            (&self.snow_ambient, &self.snow_rgb)
        };
        Some(crate::map::construction_trace::GeneratedLightingOverride {
            ambient_percent: *ambient.get(time)?,
            rgb_percent: [*rgb[0].get(time)?, *rgb[1].get(time)?, *rgb[2].get(time)?],
            level_units: *self.level_light.get(time)?,
        })
    }

    /// `[General]` through ReadInt over each field and the IntVector reader
    /// (`0x00475D70`) for the lists (`0x005982D0..0x0059877A`).
    pub(super) fn apply(&mut self, ini: &IniFile) {
        let general = ini.section_or_empty("General");
        self.min_tiberium = general.read_int("RMGMinimumTiberium", self.min_tiberium);
        self.max_tiberium = general.read_int("RMGMaximumTiberium", self.max_tiberium);
        self.max_trees = general.read_int("MaxTrees", self.max_trees);
        // 0059829D..005984DB clears these vectors before ReadINI_IntVector
        // (00475D70): keep every atoi token, including a short nonempty list.
        self.level_light = general
            .read_int_list("RMGLevelLightSettings")
            .unwrap_or_default();
        self.temperate_ambient = general
            .read_int_list("TemperateAmbientLight")
            .unwrap_or_default();
        self.snow_ambient = general
            .read_int_list("SnowAmbientLight")
            .unwrap_or_default();
        self.temperate_rgb = [
            "TemperateAmbientRed",
            "TemperateAmbientGreen",
            "TemperateAmbientBlue",
        ]
        .map(|key| general.read_int_list(key).unwrap_or_default());
        self.snow_rgb = ["SnowAmbientRed", "SnowAmbientGreen", "SnowAmbientBlue"]
            .map(|key| general.read_int_list(key).unwrap_or_default());
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

    #[test]
    fn generated_level_divides_before_double_conversion() {
        let mut settings = RmgSettings::default();
        settings.apply(&IniFile::from_str(
            "[General]\nTemperateAmbientLight=75,75,75,75\n\
             RMGLevelLightSettings=99,100,-99,-101\n",
        ));
        // Original 00599925..0059993D: signed divide-by-100 then FILD.
        for (time, expected) in [
            (0, "0.000000"),
            (1, "1.000000"),
            (2, "0.000000"),
            (3, "-1.000000"),
        ] {
            let ini = settings
                .generated_lighting_ini(&RmgOptions {
                    time,
                    ..Default::default()
                })
                .unwrap();
            let lighting = ini.section("Lighting").unwrap();
            assert_eq!(lighting.get_for_test("Level"), Some(expected));
            assert_eq!(lighting.get_for_test("Ambient"), Some("0.750000"));
        }
    }

    #[test]
    fn lighting_vectors_keep_native_tokens_and_empty_defaults() {
        let mut settings = RmgSettings::default();
        settings.apply(&IniFile::from_str(
            "[General]\nTemperateAmbientLight=75tail,,junk,-15\n\
             SnowAmbientLight=55\nRMGLevelLightSettings=3,100,-101\n",
        ));
        assert_eq!(settings.temperate_ambient, [75, 0, -15]);
        assert_eq!(settings.snow_ambient, [55]);
        let ini = settings
            .generated_lighting_ini(&RmgOptions {
                time: 2,
                ..Default::default()
            })
            .unwrap();
        let lighting = ini.section("Lighting").unwrap();
        assert_eq!(lighting.get_for_test("Ambient"), Some("-0.150000"));
        // Every nonzero theater uses Snow, not only theater 1 (00599866).
        let snow = settings
            .generated_lighting_ini(&RmgOptions {
                theater: 4,
                time: 0,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            snow.section("Lighting").unwrap().get_for_test("Ambient"),
            Some("0.550000")
        );
        assert!(
            settings
                .generated_lighting_ini(&RmgOptions {
                    time: 3,
                    ..Default::default()
                })
                .is_none()
        );
        settings.apply(&IniFile::from_str("[General]\nMaxTrees=42\n"));
        assert!(settings.temperate_ambient.is_empty());
        assert!(settings.snow_ambient.is_empty());
        assert!(settings.level_light.is_empty());
    }
}
