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
    /// VERA extension: the urban pass's lists for an opted-in Urban Presence
    /// (`tech_catalog::resolve_vera_urban`). Not from `RMGMD.INI`; the app
    /// fills it per theater. Empty places no buildings or foots.
    pub vera_urban: crate::map::rmg::phases::urban_areas::UrbanCatalog,
    /// VERA extension: `[Extras]`, what the random map's Extras place and
    /// the light of their moods.
    pub extras: super::extras::ExtrasSettings,
    /// Ares `[General] GenerateUrbanAreas=`: the process's first value of
    /// the urban-areas switch ([`RmgOptions::urban_areas`]).
    #[cfg(feature = "ares")]
    pub generate_urban_areas: bool,
    /// Ares `[Urban] Structures=`, `Infantry=` and `Vehicles=` names.
    #[cfg(feature = "ares")]
    pub urban_structures: Vec<String>,
    #[cfg(feature = "ares")]
    pub urban_infantry: Vec<String>,
    #[cfg(feature = "ares")]
    pub urban_vehicles: Vec<String>,
}

/// The `[Urban]` defaults of MapSeedClass_LoadFromINI (Ares.dll 1006ABF0).
#[cfg(feature = "ares")]
const URBAN_STRUCTURES: &str = "CABUNK01,CABUNK02,CAARMY01,CAARMY02,CAARMY03,CAARMY04,CACHIG03,\
CANEWY01,CANEWY14,CANWY09,CANWY26,CANWY25,CATEXS07";
#[cfg(feature = "ares")]
const URBAN_INFANTRY: &str = "CIV1,CIV2,CIV3,CIVA,CIVB,CIVC";
#[cfg(feature = "ares")]
const URBAN_VEHICLES: &str = "TRUCKA,TRUCKB,COP,EUROC,SUVW,SUVB,FTRK,AMBU";

/// Ares `VectorNames::Tokenize`: `strtok` on commas alone, so empty tokens
/// vanish and spaces stay in the names.
#[cfg(feature = "ares")]
fn tokenize_names(text: &str) -> Vec<String> {
    text.split(',')
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect()
}

/// `MapSeedClass::InitDefaults @ 0x00595740`.
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
            vera_urban: Default::default(),
            extras: Default::default(),
            // RMG::UrbanAreas starts false; an absent file reads the defaults.
            #[cfg(feature = "ares")]
            generate_urban_areas: false,
            #[cfg(feature = "ares")]
            urban_structures: tokenize_names(URBAN_STRUCTURES),
            #[cfg(feature = "ares")]
            urban_infantry: tokenize_names(URBAN_INFANTRY),
            #[cfg(feature = "ares")]
            urban_vehicles: tokenize_names(URBAN_VEHICLES),
        }
    }
}

impl RmgSettings {
    /// Agreement over the resolved settings consumed by generation. File
    /// selection and per-key defaults remain owned by `load` and `apply`.
    /// The Extras are offline-only, so `[Extras]` stays out.
    pub fn multiplayer_digest(&self) -> String {
        let mut bytes = b"VERA20k RMG settings v1\0".to_vec();
        for value in [self.min_tiberium, self.max_tiberium, self.max_trees]
            .into_iter()
            .chain(self.level_light.iter().copied())
            .chain(self.vegetation_min)
            .chain(self.vegetation_max)
        {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        #[cfg(feature = "ares")]
        for list in [
            &self.urban_structures,
            &self.urban_infantry,
            &self.urban_vehicles,
        ] {
            for name in list {
                bytes.extend_from_slice(name.as_bytes());
                bytes.push(b',');
            }
            bytes.push(0);
        }
        crate::util::sha256::sha256_hex(&bytes)
    }

    /// Load from the asset manager, falling back per key to the defaults above.
    /// `MapSeedClass::ReadINI @ 0x005981F0` opens `RMGMD.INI` alone
    /// (0x0082BDCC); the RA2 `RMG.INI` is never read.
    pub fn load(assets: &AssetManager) -> Self {
        let mut settings = Self::default();
        let Some(bytes) = assets.get_ref("rmgmd.ini") else {
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
    pub fn generated_lighting_ini(&self, options: &RmgOptions) -> Option<IniFile> {
        let time = usize::try_from(options.time).ok()?;
        let ambient = if options.theater == 0 {
            &self.temperate_ambient
        } else {
            &self.snow_ambient
        };
        // VERA extension: a mood's ambient stands in for the time's.
        let ambient = self
            .mood(options)
            .map_or_else(|| ambient.get(time).copied(), |mood| Some(mood.0))?;
        let ambient = TruncF64::from_f64(f64::from(ambient))
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
        let level_units = *self.level_light.get(time)?;
        // VERA extension: a mood's ambient and tint stand in for the time's;
        // the level light stays the time's.
        if let Some((ambient_percent, rgb_percent)) = self.mood(options) {
            return Some(crate::map::construction_trace::GeneratedLightingOverride {
                ambient_percent,
                rgb_percent,
                level_units,
            });
        }
        Some(crate::map::construction_trace::GeneratedLightingOverride {
            ambient_percent: *ambient.get(time)?,
            rgb_percent: [*rgb[0].get(time)?, *rgb[1].get(time)?, *rgb[2].get(time)?],
            level_units,
        })
    }

    /// VERA extension (`Extra::Mood`): the light of the mood the options
    /// choose, none at 0.
    fn mood(&self, options: &RmgOptions) -> Option<(i32, [i32; 3])> {
        usize::from(options.extras.get(super::extras::Extra::Mood))
            .checked_sub(1)
            .and_then(|index| self.extras.moods.get(index).copied())
    }

    /// `MapSeedClass::ReadINI @ 0x005981F0` reads `RMGMD.INI`
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
        // MapSeedClass_LoadFromINI (Ares.dll 1006ABF0, hook 0x5982D5, after
        // RMGMaximumTiberium): gamemd's ReadBool and 0x800-byte ReadString.
        // Ares reads them once per process; RMGMD.INI does not change, so
        // each load reads the same values.
        #[cfg(feature = "ares")]
        {
            self.generate_urban_areas =
                general.read_bool("GenerateUrbanAreas", self.generate_urban_areas);
            let urban = ini.section_or_empty("Urban");
            self.urban_structures =
                tokenize_names(&urban.read_string("Structures", URBAN_STRUCTURES, 0x800));
            self.urban_infantry =
                tokenize_names(&urban.read_string("Infantry", URBAN_INFANTRY, 0x800));
            self.urban_vehicles =
                tokenize_names(&urban.read_string("Vehicles", URBAN_VEHICLES, 0x800));
        }
        // VERA extension: gamemd reads no `[Extras]`.
        self.extras.apply(ini.section_or_empty("Extras"));
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
    fn load_reads_rmgmd_ini_only() {
        use crate::assets::asset_manager::test_support::TestDirectory;
        let ra2 = TestDirectory::new("rmg-settings-ra2-only");
        std::fs::write(ra2.path().join("rmg.ini"), "[General]\nMaxTrees=42\n").unwrap();
        let settings = RmgSettings::load(&AssetManager::from_loose_root_for_test(ra2.path()));
        assert_eq!(settings.max_trees, 500, "the RA2 RMG.INI is not a fallback");

        let yr = TestDirectory::new("rmg-settings-yr");
        std::fs::write(yr.path().join("rmgmd.ini"), "[General]\nMaxTrees=42\n").unwrap();
        let settings = RmgSettings::load(&AssetManager::from_loose_root_for_test(yr.path()));
        assert_eq!(settings.max_trees, 42);
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

    /// Ares MapSeedClass_LoadFromINI (Ares.dll 1006ABF0, hook 0x5982D5):
    /// gamemd's ReadBool and ReadString with Ares' defaults, split on commas
    /// alone. The loader drops a key with an empty value, so it keeps the
    /// default.
    #[cfg(feature = "ares")]
    #[test]
    fn urban_keys_read_with_ares_defaults_and_comma_tokens() {
        let defaults = RmgSettings::default();
        assert!(!defaults.generate_urban_areas);
        assert_eq!(
            defaults.urban_infantry,
            ["CIV1", "CIV2", "CIV3", "CIVA", "CIVB", "CIVC"]
        );
        assert_eq!(defaults.urban_structures.len(), 13);
        assert_eq!(defaults.urban_structures[12], "CATEXS07");

        let mut settings = RmgSettings::default();
        settings.apply(&IniFile::from_str(
            "[General]\nGenerateUrbanAreas=yes\n\
             [Urban]\nStructures=CAX,,CA Y\nVehicles=\n",
        ));
        assert!(settings.generate_urban_areas);
        assert_eq!(settings.urban_structures, ["CAX", "CA Y"]);
        assert_eq!(settings.urban_vehicles, defaults.urban_vehicles);
        assert_eq!(settings.urban_infantry, defaults.urban_infantry);
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

    /// VERA extension: a mood lights the map in place of its time of day,
    /// keeping the time's level light; with no mood the time's light stands.
    #[test]
    fn a_mood_stands_in_for_the_light_of_the_time_of_day() {
        use crate::map::rmg::extras::Extra;
        let mut settings = RmgSettings::default();
        settings.apply(&IniFile::from_str(
            "[General]\nTemperateAmbientLight=75,100\nRMGLevelLightSettings=3,4\n\
             TemperateAmbientRed=109,108\nTemperateAmbientGreen=80,94\n\
             TemperateAmbientBlue=49,68\n",
        ));
        let mut options = RmgOptions {
            time: 1,
            ..Default::default()
        };
        let plain = settings.generated_lighting_override(&options).unwrap();
        assert_eq!(
            (plain.ambient_percent, plain.rgb_percent, plain.level_units),
            (100, [108, 94, 68], 4)
        );
        options.extras.set(Extra::Mood, 2);
        let dusk = settings.generated_lighting_override(&options).unwrap();
        assert_eq!(
            (dusk.ambient_percent, dusk.rgb_percent),
            (62, [135, 70, 55])
        );
        assert_eq!(dusk.level_units, 4);
        let ini = settings.generated_lighting_ini(&options).unwrap();
        assert_eq!(
            ini.section("Lighting").unwrap().get_for_test("Ambient"),
            Some("0.620000")
        );
    }

    /// VERA's `[Extras]` keys name what the Extras place and set a mood's
    /// light; an absent key, or a mood short of a number, keeps the default.
    #[test]
    fn extras_keys_replace_the_names_and_the_mood_light() {
        use crate::map::rmg::extras::{Extra, ExtrasSettings};
        let mut settings = RmgSettings::default();
        settings.apply(&IniFile::from_str(
            "[Extras]\nFarms=CABARN02, CAFARM01\nFairTech=CAMACH\nTownFence=CAFNCG\n\
             RedDusk=50,140,60,40\nMoonlight=40,50\n",
        ));
        let (read, defaults) = (&settings.extras, ExtrasSettings::default());
        assert_eq!(read.farms, ["CABARN02", "CAFARM01"]);
        assert_eq!(
            (read.fair_tech.as_str(), read.town_fence.as_str()),
            ("CAMACH", "CAFNCG")
        );
        assert_eq!(
            (&read.landmarks, &read.farm_fence),
            (&defaults.landmarks, &defaults.farm_fence)
        );
        assert_eq!(read.moods[2], defaults.moods[2], "a short mood is refused");
        let options = RmgOptions {
            extras: {
                let mut extras = crate::map::rmg::extras::RmgExtras::default();
                extras.set(Extra::Mood, 2);
                extras
            },
            ..Default::default()
        };
        assert_eq!(settings.mood(&options), Some((50, [140, 60, 40])));
    }
}
